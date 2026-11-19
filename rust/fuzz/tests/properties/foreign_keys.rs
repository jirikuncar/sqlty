//! `foreign-key-mismatch` and `entity-mismatch`: key comparisons in random query shapes. An independent oracle
//! judges each comparison from the schema; the rule must report exactly what it does,
//! wherever the comparison sits: `ON`, `WHERE`, `HAVING`, joins of every kind, `LATERAL`,
//! subqueries, CTEs, derived tables, `UNION` branches.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use proptest::prelude::*;
use sqlty_sql::analyzer::Schema;

use crate::draw::{check, Draw};
use crate::{engine, python};

const SCHEMA: &str = "
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER REFERENCES customers (id),
    product_id INTEGER REFERENCES products (id),
    total INTEGER
);
CREATE TABLE vip (customer_id INTEGER PRIMARY KEY REFERENCES customers (id));
CREATE TABLE vip_notes (id INTEGER PRIMARY KEY, vip_id INTEGER REFERENCES vip (customer_id));
CREATE TABLE employees (id INTEGER PRIMARY KEY, manager_id INTEGER REFERENCES employees (id));
CREATE TABLE events (id BIGINT, customer_id INTEGER, product_id INTEGER);
CREATE TABLE warehouses (region TEXT, code TEXT, PRIMARY KEY (region, code));
CREATE TABLE stock (
    id INTEGER PRIMARY KEY,
    region TEXT,
    code TEXT,
    FOREIGN KEY (region, code) REFERENCES warehouses (region, code)
);
";
/// Tables and their columns, in name order.
const COLUMNS: [(&str, &[&str]); 7] = [
    ("customers", &["id", "name"]),
    ("employees", &["id", "manager_id"]),
    ("events", &["id", "customer_id", "product_id"]),
    ("orders", &["id", "customer_id", "product_id", "total"]),
    ("products", &["id", "name"]),
    ("vip", &["customer_id"]),
    ("vip_notes", &["id", "vip_id"]),
];
const KEYS: [(Column, Column); 5] = [
    (("orders", "customer_id"), ("customers", "id")),
    (("orders", "product_id"), ("products", "id")),
    (("vip", "customer_id"), ("customers", "id")),
    (("vip_notes", "vip_id"), ("vip", "customer_id")),
    (("employees", "manager_id"), ("employees", "id")),
];

type Column = (&'static str, &'static str);
type Reports = BTreeSet<String>;

static DIALECT_SCHEMAS: LazyLock<HashMap<&str, Schema>> = LazyLock::new(|| {
    ["postgres", "duckdb", "sqlite", "mysql"]
        .into_iter()
        .map(|dialect| (dialect, engine::schema(SCHEMA, dialect)))
        .collect()
});

fn columns(table: &str) -> &'static [&'static str] {
    COLUMNS
        .iter()
        .find(|(name, _)| *name == table)
        .map_or(&[], |(_, columns)| columns)
}

fn key(column: Column) -> Option<Column> {
    KEYS.iter()
        .find(|(from, _)| *from == column)
        .map(|(_, to)| *to)
}

fn primary(column: Column) -> bool {
    match column {
        ("vip", name) => name == "customer_id",
        ("events", _) => false,
        (_, name) => name == "id",
    }
}

/// The key column a column's values are keys of, following foreign keys to the end.
fn entity(mut column: Column) -> Option<Column> {
    while let Some(target) = key(column) {
        column = target;
    }
    primary(column).then_some(column)
}

/// What the rule reports for `left` compared with `right`, by the rule page.
fn expected(left: Column, right: Column) -> Option<String> {
    if key(left).is_none() && key(right).is_none() {
        return None;
    }
    let same_keys = key(left).is_some() && key(left) == key(right);
    let namesake = left.1 == right.1
        && [left, right]
            .iter()
            .any(|side| key(*side).is_none() && entity(*side).is_none());
    if left == right
        || key(left) == Some(right)
        || key(right) == Some(left)
        || same_keys
        || entity(left).is_some() && entity(left) == entity(right)
        || namesake
    {
        return None;
    }
    let (from, other) = if key(left).is_some() {
        (left, right)
    } else {
        (right, left)
    };
    let target = key(from)?;
    Some(format!(
        "{}.{} references {}.{}, not {}.{}",
        from.0, from.1, target.0, target.1, other.0, other.1
    ))
}

/// What both rules report for `left` compared with `right`: `entity-mismatch` only where
/// `foreign-key-mismatch` is silent.
fn reports_for(left: Column, right: Column) -> Reports {
    if let Some(message) = expected(left, right) {
        return Reports::from([message]);
    }
    let (Some(a), Some(b)) = (entity(left), entity(right)) else {
        return Reports::new();
    };
    if a == b {
        return Reports::new();
    }
    let named = |e: Column| {
        if a.0 == b.0 {
            format!("{}.{}", e.0, e.1)
        } else {
            e.0.to_string()
        }
    };
    Reports::from([format!(
        "{}.{} holds {} keys, {}.{} holds {} keys",
        left.0,
        left.1,
        named(a),
        right.0,
        right.1,
        named(b)
    )])
}

const NOISE: [&str; 4] = [
    "{x}.id > 0",
    "{y}.id IS NOT NULL",
    "1 = 1",
    "{x}.id IN (1, 2)",
];
/// Each form, and whether its left operand is `b`.
const FORMS: [(&str, bool); 10] = [
    ("{a} = {b}", false),
    ("{b} = {a}", true),
    ("{a} <> {b}", false),
    ("({a} = {b})", false),
    ("NOT ({a} = {b})", false),
    ("({a}, 1) = ({b}, 1)", false),
    ("(1, {b}) = (1, {a})", true),
    ("{a} IN ({b}, 3)", false),
    ("{a} IN ({b})", false),
    ("{a} NOT IN ({b})", false),
];

fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

fn noise(template: &str, x: &str, y: &str) -> String {
    fill(template, &[("x", x), ("y", y)])
}

fn joined(d: &mut Draw<'_>, terms: Vec<String>, ops: &[&str]) -> String {
    let terms = d.shuffle(terms);
    let mut cond = terms[0].clone();
    for term in &terms[1..] {
        cond = format!("{cond} {} {term}", d.pick(ops));
    }
    cond
}

/// A query `SELECT 1 ...` comparing source `x` of `tx` with `y` of `ty`.
fn shapes(x: &str, y: &str, tx: &str, ty: &str, cond: &str) -> Vec<String> {
    let mut shapes: Vec<String> = ["JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN"]
        .iter()
        .map(|kind| format!("SELECT 1 FROM {tx} {x} {kind} {ty} {y} ON {cond}"))
        .collect();
    shapes.extend([
        format!("SELECT 1 FROM {tx} {x}, {ty} {y} WHERE {cond}"),
        format!("SELECT 1 FROM {tx} {x} CROSS JOIN {ty} {y} WHERE {cond}"),
        format!("SELECT 1 FROM {tx} {x} JOIN {ty} {y} ON true WHERE {cond}"),
        format!("SELECT 1 FROM {tx} {x}, {ty} {y} GROUP BY {x}.id, {y}.id HAVING {cond}"),
        format!("SELECT 1 FROM {tx} {x} WHERE EXISTS (SELECT 1 FROM {ty} {y} WHERE {cond})"),
        format!("SELECT 1 FROM {tx} {x} WHERE NOT EXISTS (SELECT 1 FROM {ty} {y} WHERE {cond})"),
        format!("SELECT 1 FROM {tx} {x} WHERE 1 IN (SELECT 1 FROM {ty} {y} WHERE {cond})"),
        format!("SELECT 1 FROM {tx} {x} JOIN LATERAL (SELECT 1 AS one FROM {ty} {y} WHERE {cond}) l{y} ON true"),
        format!("SELECT 1 FROM {tx} {x} CROSS JOIN LATERAL (SELECT 1 AS one FROM {ty} {y} WHERE {cond}) l{y}"),
        format!("SELECT (SELECT 1 FROM {ty} {y} WHERE {cond} LIMIT 1) FROM {tx} {x}"),
        format!("SELECT 1 FROM {tx} {x} JOIN {ty} {y} ON EXISTS (SELECT 1 WHERE {cond})"),
        format!("SELECT 1 FROM {tx} {x} JOIN {ty} {y} ON true WHERE {x}.id > 0 AND ({cond})"),
    ]);
    shapes
}

const WRAPPERS: [&str; 10] = [
    "{q}",
    "WITH c{n} AS ({q}) SELECT 1 FROM c{n}",
    "WITH RECURSIVE r{n} AS ({q} UNION ALL SELECT 1 FROM r{n} WHERE false) SELECT 1 FROM r{n}",
    "SELECT 1 FROM ({q}) d{n}",
    "SELECT 1 FROM customers z{n} JOIN LATERAL ({q}) d{n} ON true",
    "SELECT 1 WHERE EXISTS ({q})",
    "SELECT 1 FROM customers z{n} WHERE 1 IN ({q})",
    "SELECT ({q} LIMIT 1)",
    "{q} UNION ALL SELECT 1",
    "SELECT 1 UNION SELECT * FROM ({q}) u{n}",
];

fn nest(d: &mut Draw<'_>, mut sql: String) -> String {
    for n in 0..d.int(0, 2) {
        sql = fill(d.pick(&WRAPPERS), &[("n", &n.to_string()), ("q", &sql)]);
    }
    sql
}

/// Names are judged where the dialect's name rules are modelled: not in DuckDB.
const JUDGED: [&str; 3] = ["postgres", "sqlite", "mysql"];
const DIALECTS: [&str; 4] = ["postgres", "duckdb", "sqlite", "mysql"];

fn supported(sql: &str, dialect: &str) -> bool {
    let unsupported: &[&str] = match dialect {
        "sqlite" => &["LATERAL", "RIGHT JOIN", "FULL JOIN"],
        "mysql" => &["FULL JOIN"],
        _ => &[],
    };
    !unsupported.iter().any(|word| sql.contains(word))
}

fn found(sql: &str, dialect: &'static str) -> Result<Reports, TestCaseError> {
    let marker = if dialect == "mysql" {
        "-- sql"
    } else {
        "--sql"
    };
    let source = format!("q = {}\n", python::repr(&format!("{marker}\n{sql}")));
    let rules = [("entity-mismatch", "warn")];
    let diagnostics = engine::check_with(&source, dialect, &DIALECT_SCHEMAS[dialect], None, &rules);
    prop_assert!(
        !diagnostics
            .iter()
            .any(|(rule, _)| *rule == "internal-error"),
        "{}",
        sql
    );
    Ok(diagnostics
        .into_iter()
        .filter(|(rule, _)| matches!(*rule, "foreign-key-mismatch" | "entity-mismatch"))
        .map(|(_, message)| message)
        .collect())
}

/// The reports for `sql` in `dialect`: those of the oracle where names are judged.
fn judged(sql: &str, dialect: &'static str, reports: Reports) -> Result<(), TestCaseError> {
    if !supported(sql, dialect) {
        return Err(TestCaseError::reject("the dialect has no such join"));
    }
    let wanted = if JUDGED.contains(&dialect) {
        reports
    } else {
        Reports::new()
    };
    prop_assert_eq!(found(sql, dialect)?, wanted, "{}: {}", dialect, sql);
    Ok(())
}

fn table(d: &mut Draw<'_>) -> &'static str {
    d.pick(&COLUMNS).0
}

/// The tables are drawn first, so the columns compared belong to them.
fn comparison_of_tables(d: &mut Draw<'_>) -> (String, Reports) {
    let (tx, ty) = (table(d), table(d));
    let pairs = d.list(1, 3, |d| (d.pick(columns(tx)), d.pick(columns(ty))));
    let mut terms = Vec::new();
    let mut reports = Reports::new();
    for (a, b) in pairs {
        let (form, swapped) = d.pick(&FORMS);
        terms.push(fill(
            form,
            &[("a", &format!("x.{a}")), ("b", &format!("y.{b}"))],
        ));
        let (left, right) = if swapped {
            ((ty, b), (tx, a))
        } else {
            ((tx, a), (ty, b))
        };
        reports.extend(reports_for(left, right));
    }
    terms.extend(d.list(0, 2, |d| noise(d.pick(&NOISE), "x", "y")));
    let cond = joined(d, terms, &["AND", "OR"]);
    // A derived table hides the keys of its table: nothing is judged.
    let source = d.pick(&[
        "{t}",
        "{t}",
        "{t}",
        "(SELECT * FROM {t})",
        "(SELECT {t}.* FROM {t})",
    ]);
    if source != "{t}" {
        reports.clear();
    }
    let sql = d.pick(&shapes("x", "y", tx, &fill(source, &[("t", ty)]), &cond));
    (nest(d, sql), reports)
}

#[test]
fn single_column_keys_in_any_shape() {
    check(
        |d| (comparison_of_tables(d), d.pick(&DIALECTS)),
        |((sql, reports), dialect)| judged(&sql, dialect, reports),
    );
}

/// `stock` compared with one `warehouses` row on some of its key, in any shape.
fn composite(d: &mut Draw<'_>) -> (String, Reports) {
    let mut columns = d.subset(&["code", "region"], 1, 2);
    columns.sort_unstable();
    let pinned: Vec<&str> = if d.bool() {
        ["code", "region"]
            .into_iter()
            .filter(|c| !columns.contains(c))
            .collect()
    } else {
        vec![]
    };
    let mut terms: Vec<String> = columns.iter().map(|c| format!("s.{c} = w.{c}")).collect();
    terms.extend(
        pinned
            .iter()
            .map(|c| format!("s.{c} = 'k' AND w.{c} = 'k'")),
    );
    terms.extend(d.list(0, 2, |d| noise(d.pick(&NOISE), "s", "w")));
    let cond = joined(d, terms, &["AND"]);
    let sql = d.pick(&shapes("s", "w", "stock", "warehouses", &cond));
    let sql = nest(d, sql);
    let reports = if columns.len() + pinned.len() == 2 {
        Reports::new()
    } else {
        Reports::from([format!(
            "stock (region, code) references warehouses (region, code), compared with w on {} only",
            columns.join(", ")
        )])
    };
    (sql, reports)
}

#[test]
fn composite_keys_in_any_shape() {
    check(composite, |(sql, reports)| {
        prop_assert_eq!(found(&sql, "postgres")?, reports, "{}", sql);
        Ok(())
    });
}

const JOINS: [&str; 7] = [
    "JOIN",
    "INNER JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "CROSS JOIN",
    ",",
];

fn compare(d: &mut Draw<'_>, visible: &[(String, &'static str)], reports: &mut Reports) -> String {
    let (x, tx) = d.pick(visible);
    let (y, ty) = d.pick(visible);
    let (a, b) = (d.pick(columns(tx)), d.pick(columns(ty)));
    let (form, swapped) = d.pick(&FORMS);
    let (left, right) = if swapped {
        ((ty, b), (tx, a))
    } else {
        ((tx, a), (ty, b))
    };
    reports.extend(reports_for(left, right));
    fill(
        form,
        &[("a", &format!("{x}.{a}")), ("b", &format!("{y}.{b}"))],
    )
}

/// Three to five sources joined in a chain, each condition comparing sources before it,
/// with `USING` where one source before has the column.
fn join_chain(d: &mut Draw<'_>) -> (String, Reports) {
    let count = d.int(3, 5);
    let sources: Vec<(String, &str)> = (0..count).map(|i| (format!("s{i}"), table(d))).collect();
    let mut reports = Reports::new();
    let mut sql = format!("SELECT 1 FROM {} {}", sources[0].1, sources[0].0);
    let mut wheres = Vec::new();
    for (i, (alias, table)) in sources.iter().enumerate().skip(1) {
        let visible = &sources[..=i];
        let kind = d.pick(&JOINS);
        let shared: Vec<&str> = columns(table)
            .iter()
            .copied()
            .filter(|column| {
                sources[..i]
                    .iter()
                    .filter(|(_, t)| columns(t).contains(column))
                    .count()
                    == 1
            })
            .collect();
        if kind == "," || kind == "CROSS JOIN" {
            sql += &format!(
                "{}{kind} {table} {alias}",
                if kind == "CROSS JOIN" { " " } else { "" }
            );
        } else if !shared.is_empty() && d.bool() {
            let column = d.pick(&shared);
            let other = sources[..i]
                .iter()
                .find(|(_, t)| columns(t).contains(&column))
                .map_or("", |(_, t)| t);
            reports.extend(reports_for((other, column), (table, column)));
            sql += &format!(" {kind} {table} {alias} USING ({column})");
        } else {
            let terms = d.list(1, 2, |d| compare(d, visible, &mut reports));
            sql += &format!(
                " {kind} {table} {alias} ON {}",
                joined(d, terms, &["AND", "OR"])
            );
        }
        if d.bool() {
            wheres.push(compare(d, visible, &mut reports));
        }
    }
    if !wheres.is_empty() {
        sql += &format!(" WHERE {}", joined(d, wheres, &["AND", "OR"]));
    }
    (nest(d, sql), reports)
}

#[test]
fn join_chains() {
    check(
        |d| (join_chain(d), d.pick(&DIALECTS)),
        |((sql, reports), dialect)| judged(&sql, dialect, reports),
    );
}

const KEY: [&str; 2] = ["region", "code"];

/// Each `stock` key column compared with a column of `w` or `v`, pinned, NULL or left out,
/// and the reports the rule page gives for that.
fn composite_rows(d: &mut Draw<'_>) -> (String, Reports) {
    let (mut pairs, mut pins, mut nulls, mut terms) =
        (Vec::new(), BTreeSet::new(), false, Vec::new());
    for column in KEY {
        match d.pick(&["compare", "pin", "pin-other", "null", "none"]) {
            "compare" => {
                let (alias, other) = (d.pick(&["w", "v"]), d.pick(&KEY));
                pairs.push((column, alias, other));
                let form = d.pick(&["s.{c} = {a}.{o}", "{a}.{o} = s.{c}", "(s.{c}) IN ({a}.{o})"]);
                terms.push(fill(form, &[("c", column), ("a", alias), ("o", other)]));
            }
            way @ ("pin" | "pin-other") => {
                let value = d.pick(&["'k'", "$1", "2"]);
                let other = if way == "pin" {
                    value
                } else {
                    d.pick(&["'j'", "$2", "NULL"])
                };
                terms.push(format!("s.{column} = {value} AND w.{column} = {other}"));
                if way == "pin" || other == value {
                    pins.insert(column);
                }
            }
            "null" => {
                terms.push(format!("s.{column} IS NULL"));
                nulls = true;
            }
            _ => {}
        }
    }
    terms.extend(d.list(0, 1, |d| noise(d.pick(&NOISE[2..]), "s", "w")));
    let mut reports = Reports::new();
    let straight: Vec<(&str, &str)> = pairs
        .iter()
        .filter(|(c, _, o)| c == o)
        .map(|(c, a, _)| (*c, *a))
        .collect();
    for (column, _, other) in &pairs {
        if column != other {
            reports.insert(format!(
                "stock.{column} references warehouses.{column}, not warehouses.{other}"
            ));
        }
    }
    let rows: BTreeSet<&str> = straight.iter().map(|(_, a)| *a).collect();
    if rows.len() > 1 {
        let compared: Vec<String> = straight.iter().map(|(c, a)| format!("{a}.{c}")).collect();
        reports.insert(format!(
            "stock (region, code) references one warehouses row, compared with {}",
            compared.join(" and ")
        ));
    } else if let (Some(alias), false) = (rows.first(), nulls) {
        let columns: BTreeSet<&str> = straight.iter().map(|(c, _)| *c).collect();
        let pinned = if *alias == "w" {
            pins.clone()
        } else {
            BTreeSet::new()
        };
        if columns.union(&pinned).count() != KEY.len() {
            let only: Vec<&str> = columns.into_iter().collect();
            reports.insert(format!(
                "stock (region, code) references warehouses (region, code), compared with {alias} on {} only",
                only.join(", ")
            ));
        }
    }
    let mut cond = if terms.is_empty() {
        "true".to_string()
    } else {
        terms.join(" AND ")
    };
    if d.bool() {
        cond = format!("({cond}) OR {}", d.pick(&["1 = 2", "s.id IS NULL"]));
    }
    let sql = format!("SELECT 1 FROM stock s, warehouses w, warehouses v WHERE {cond}");
    (nest(d, sql), reports)
}

#[test]
fn composite_keys_across_rows() {
    check(composite_rows, |(sql, reports)| {
        prop_assert_eq!(found(&sql, "postgres")?, reports, "{}", sql);
        Ok(())
    });
}
