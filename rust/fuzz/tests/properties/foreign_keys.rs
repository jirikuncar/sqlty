//! `foreign-key-mismatch` and `entity-mismatch`: key comparisons in random query shapes.
//! An independent oracle judges each comparison from the schema; the rules must report
//! exactly what it does, wherever the comparison sits: `ON`, `WHERE`, `HAVING`, joins of
//! every kind, `LATERAL`, subqueries, CTEs, derived tables, `UNION` branches.

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
type Column = (&'static str, &'static str);
type Reports = BTreeSet<String>;

/// Tables and their columns, in name order.
#[rustfmt::skip]
const COLUMNS: [(&str, &[&str]); 7] = [
    ("customers", &["id", "name"]), ("employees", &["id", "manager_id"]),
    ("events", &["id", "customer_id", "product_id"]), ("orders", &["id", "customer_id", "product_id", "total"]),
    ("products", &["id", "name"]), ("vip", &["customer_id"]), ("vip_notes", &["id", "vip_id"]),
];
#[rustfmt::skip]
const KEYS: [(Column, Column); 5] = [
    (("orders", "customer_id"), ("customers", "id")), (("orders", "product_id"), ("products", "id")),
    (("vip", "customer_id"), ("customers", "id")), (("vip_notes", "vip_id"), ("vip", "customer_id")),
    (("employees", "manager_id"), ("employees", "id")),
];
const NOISE: [&str; 4] = [
    "{x}.id > 0",
    "{y}.id IS NOT NULL",
    "1 = 1",
    "{x}.id IN (1, 2)",
];
/// Each form, and whether its left operand is `b`.
#[rustfmt::skip]
const FORMS: [(&str, bool); 10] = [
    ("{a} = {b}", false), ("{b} = {a}", true), ("{a} <> {b}", false), ("({a} = {b})", false),
    ("NOT ({a} = {b})", false), ("({a}, 1) = ({b}, 1)", false), ("(1, {b}) = (1, {a})", true),
    ("{a} IN ({b}, 3)", false), ("{a} IN ({b})", false), ("{a} NOT IN ({b})", false),
];
#[rustfmt::skip]
const WRAPPERS: [&str; 10] = [
    "{q}", "WITH c{n} AS ({q}) SELECT 1 FROM c{n}",
    "WITH RECURSIVE r{n} AS ({q} UNION ALL SELECT 1 FROM r{n} WHERE false) SELECT 1 FROM r{n}",
    "SELECT 1 FROM ({q}) d{n}", "SELECT 1 FROM customers z{n} JOIN LATERAL ({q}) d{n} ON true",
    "SELECT 1 WHERE EXISTS ({q})", "SELECT 1 FROM customers z{n} WHERE 1 IN ({q})", "SELECT ({q} LIMIT 1)",
    "{q} UNION ALL SELECT 1", "SELECT 1 UNION SELECT * FROM ({q}) u{n}",
];
const JOINS: [&str; 7] = [
    "JOIN",
    "INNER JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "CROSS JOIN",
    ",",
];
const DIALECTS: [&str; 4] = ["postgres", "duckdb", "sqlite", "mysql"];
/// Names are judged where the dialect's name rules are modelled: not in DuckDB.
const JUDGED: [&str; 3] = ["postgres", "sqlite", "mysql"];

static SCHEMAS: LazyLock<HashMap<&str, Schema>> = LazyLock::new(|| {
    DIALECTS
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

/// The key column a column's values are keys of, following foreign keys to the end.
fn entity(mut column: Column) -> Option<Column> {
    while let Some(target) = key(column) {
        column = target;
    }
    let primary = match column {
        ("vip", name) => name == "customer_id",
        (table, name) => table != "events" && name == "id",
    };
    primary.then_some(column)
}

fn dotted((table, column): Column) -> String {
    format!("{table}.{column}")
}

/// What `foreign-key-mismatch` reports for `left` compared with `right`, by its rule page.
fn expected(left: Column, right: Column) -> Option<String> {
    let namesake = left.1 == right.1
        && [left, right]
            .iter()
            .any(|side| key(*side).is_none() && entity(*side).is_none());
    if key(left).is_none() && key(right).is_none()
        || left == right
        || key(left) == Some(right)
        || key(right) == Some(left)
        || key(left).is_some() && key(left) == key(right)
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
    Some(format!(
        "{} references {}, not {}",
        dotted(from),
        dotted(key(from)?),
        dotted(other)
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
    let named = |e: Column| {
        if a.0 == b.0 {
            dotted(e)
        } else {
            e.0.to_string()
        }
    };
    let message = format!(
        "{} holds {} keys, {} holds {} keys",
        dotted(left),
        named(a),
        dotted(right),
        named(b)
    );
    (a != b).then_some(message).into_iter().collect()
}

/// `form` comparing `x.a` with `y.b`, and the reports for it.
fn comparison(
    d: &mut Draw<'_>,
    (x, tx): (&str, &'static str),
    (y, ty): (&str, &'static str),
) -> (String, Reports) {
    let (a, b) = (d.pick(columns(tx)), d.pick(columns(ty)));
    let (form, swapped) = d.pick(&FORMS);
    let reports = if swapped {
        reports_for((ty, b), (tx, a))
    } else {
        reports_for((tx, a), (ty, b))
    };
    (
        form.replace("{a}", &format!("{x}.{a}"))
            .replace("{b}", &format!("{y}.{b}")),
        reports,
    )
}

fn noise(d: &mut Draw<'_>, noise: &[&str], high: usize, x: &str, y: &str) -> Vec<String> {
    d.list(0, high, |d| {
        d.pick(noise).replace("{x}", x).replace("{y}", y)
    })
}

fn joined(d: &mut Draw<'_>, terms: Vec<String>, ops: &[&str]) -> String {
    let mut terms = d.shuffle(terms).into_iter();
    let first = terms.next().unwrap_or_default();
    terms.fold(first, |cond, term| format!("{cond} {} {term}", d.pick(ops)))
}

/// A query `SELECT 1 ...` comparing source `x` of `tx` with `y` of `ty`.
fn shape(d: &mut Draw<'_>, x: &str, y: &str, tx: &str, ty: &str, cond: &str) -> String {
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
    let sql = d.pick(&shapes);
    nest(d, sql)
}

fn nest(d: &mut Draw<'_>, sql: String) -> String {
    (0..d.int(0, 2)).fold(sql, |sql, n| {
        d.pick(&WRAPPERS)
            .replace("{n}", &n.to_string())
            .replace("{q}", &sql)
    })
}

fn found(sql: &str, dialect: &'static str) -> Result<Reports, TestCaseError> {
    let marker = if dialect == "mysql" {
        "-- sql"
    } else {
        "--sql"
    };
    let source = format!("q = {}\n", python::literal(&format!("{marker}\n{sql}")));
    let rules = [("entity-mismatch", "warn")];
    let diagnostics = engine::check_with(&source, dialect, &SCHEMAS[dialect], None, &rules);
    prop_assert!(
        !diagnostics
            .iter()
            .any(|(rule, _)| *rule == "internal-error"),
        "{}",
        sql
    );
    let keys = diagnostics
        .into_iter()
        .filter(|(rule, _)| matches!(*rule, "foreign-key-mismatch" | "entity-mismatch"));
    Ok(keys.map(|(_, message)| message).collect())
}

/// The reports for `sql` in `dialect`: those of the oracle where names are judged.
fn judged(sql: &str, dialect: &'static str, reports: Reports) -> Result<(), TestCaseError> {
    let unsupported: &[&str] = match dialect {
        "sqlite" => &["LATERAL", "RIGHT JOIN", "FULL JOIN"],
        "mysql" => &["FULL JOIN"],
        _ => &[],
    };
    if unsupported.iter().any(|word| sql.contains(word)) {
        return Err(TestCaseError::reject("the dialect has no such join"));
    }
    let wanted: Reports = reports
        .into_iter()
        .filter(|_| JUDGED.contains(&dialect))
        .collect();
    prop_assert_eq!(found(sql, dialect)?, wanted, "{}: {}", dialect, sql);
    Ok(())
}

/// The tables are drawn first, so the columns compared belong to them.
fn comparison_of_tables(d: &mut Draw<'_>) -> (String, Reports) {
    let (tx, ty) = (d.pick(&COLUMNS).0, d.pick(&COLUMNS).0);
    let (mut terms, mut reports): (Vec<String>, Vec<Reports>) = d
        .list(1, 3, |d| comparison(d, ("x", tx), ("y", ty)))
        .into_iter()
        .unzip();
    terms.extend(noise(d, &NOISE, 2, "x", "y"));
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
    (
        shape(d, "x", "y", tx, &source.replace("{t}", ty), &cond),
        reports.into_iter().flatten().collect(),
    )
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
    let pinned: Vec<&str> = ["code", "region"]
        .into_iter()
        .filter(|c| !columns.contains(c))
        .collect();
    let pinned = if d.bool() { pinned } else { vec![] };
    let mut terms: Vec<String> = columns.iter().map(|c| format!("s.{c} = w.{c}")).collect();
    terms.extend(
        pinned
            .iter()
            .map(|c| format!("s.{c} = 'k' AND w.{c} = 'k'")),
    );
    terms.extend(noise(d, &NOISE, 2, "s", "w"));
    let cond = joined(d, terms, &["AND"]);
    let sql = shape(d, "s", "w", "stock", "warehouses", &cond);
    let message = format!(
        "stock (region, code) references warehouses (region, code), compared with w on {} only",
        columns.join(", ")
    );
    (
        sql,
        (columns.len() + pinned.len() < 2)
            .then_some(message)
            .into_iter()
            .collect(),
    )
}

#[test]
fn composite_keys_in_any_shape() {
    check(composite, |(sql, reports)| {
        prop_assert_eq!(found(&sql, "postgres")?, reports, "{}", sql);
        Ok(())
    });
}

/// Three to five sources joined in a chain, each condition comparing sources before it,
/// with `USING` where one source before has the column.
fn join_chain(d: &mut Draw<'_>) -> (String, Reports) {
    let sources: Vec<(String, &str)> = (0..d.int(3, 5))
        .map(|i| (format!("s{i}"), d.pick(&COLUMNS).0))
        .collect();
    let compare = |d: &mut Draw<'_>, visible: &[(String, &'static str)]| {
        let ((x, tx), (y, ty)) = (d.pick(visible), d.pick(visible));
        comparison(d, (&x, tx), (&y, ty))
    };
    let mut reports = Reports::new();
    let mut sql = format!("SELECT 1 FROM {} {}", sources[0].1, sources[0].0);
    let mut wheres = Vec::new();
    for (i, (alias, table)) in sources.iter().enumerate().skip(1) {
        let visible = &sources[..=i];
        let holding = |column: &'static str| {
            sources[..i]
                .iter()
                .filter(move |(_, t)| columns(t).contains(&column))
                .map(|(_, t)| *t)
        };
        let shared: Vec<&str> = columns(table)
            .iter()
            .copied()
            .filter(|column| holding(column).count() == 1)
            .collect();
        match d.pick(&JOINS) {
            "," => sql += &format!(", {table} {alias}"),
            "CROSS JOIN" => sql += &format!(" CROSS JOIN {table} {alias}"),
            kind if !shared.is_empty() && d.bool() => {
                let column = d.pick(&shared);
                let other = holding(column).next().unwrap_or_default();
                reports.extend(reports_for((other, column), (table, column)));
                sql += &format!(" {kind} {table} {alias} USING ({column})");
            }
            kind => {
                let (terms, found): (Vec<_>, Vec<_>) =
                    d.list(1, 2, |d| compare(d, visible)).into_iter().unzip();
                reports.extend(found.into_iter().flatten());
                sql += &format!(
                    " {kind} {table} {alias} ON {}",
                    joined(d, terms, &["AND", "OR"])
                );
            }
        }
        if d.bool() {
            let (term, found) = compare(d, visible);
            reports.extend(found);
            wheres.push(term);
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
                terms.push(
                    form.replace("{c}", column)
                        .replace("{a}", alias)
                        .replace("{o}", other),
                );
            }
            "null" => {
                terms.push(format!("s.{column} IS NULL"));
                nulls = true;
            }
            "none" => {}
            way => {
                let value = d.pick(&["'k'", "$1", "2"]);
                let other = if way == "pin" {
                    value
                } else {
                    d.pick(&["'j'", "$2", "NULL"])
                };
                terms.push(format!("s.{column} = {value} AND w.{column} = {other}"));
                if other == value {
                    pins.insert(column);
                }
            }
        }
    }
    terms.extend(noise(d, &NOISE[2..], 1, "s", "w"));
    let mut reports: Reports = pairs
        .iter()
        .filter(|(column, _, other)| column != other)
        .map(|(column, _, other)| {
            format!("stock.{column} references warehouses.{column}, not warehouses.{other}")
        })
        .collect();
    let straight: Vec<(&str, &str)> = pairs
        .iter()
        .filter(|(c, _, o)| c == o)
        .map(|(c, a, _)| (*c, *a))
        .collect();
    let rows: BTreeSet<&str> = straight.iter().map(|(_, alias)| *alias).collect();
    let columns: BTreeSet<&str> = straight.iter().map(|(column, _)| *column).collect();
    if rows.len() > 1 {
        let compared: Vec<String> = straight.iter().map(|(c, a)| format!("{a}.{c}")).collect();
        reports.insert(format!(
            "stock (region, code) references one warehouses row, compared with {}",
            compared.join(" and ")
        ));
    } else if let (Some(alias), false) = (rows.first(), nulls) {
        let pinned = if *alias == "w" { pins } else { BTreeSet::new() };
        if columns.union(&pinned).count() != KEY.len() {
            let only = columns.into_iter().collect::<Vec<_>>().join(", ");
            reports.insert(format!(
                "stock (region, code) references warehouses (region, code), compared with {alias} on {only} only"
            ));
        }
    }
    let mut cond = if terms.is_empty() {
        "true".into()
    } else {
        terms.join(" AND ")
    };
    if d.bool() {
        cond = format!("({cond}) OR {}", d.pick(&["1 = 2", "s.id IS NULL"]));
    }
    (
        nest(
            d,
            format!("SELECT 1 FROM stock s, warehouses w, warehouses v WHERE {cond}"),
        ),
        reports,
    )
}

#[test]
fn composite_keys_across_rows() {
    check(composite_rows, |(sql, reports)| {
        prop_assert_eq!(found(&sql, "postgres")?, reports, "{}", sql);
        Ok(())
    });
}
