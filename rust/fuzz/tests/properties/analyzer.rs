//! Random schemas and queries through the analyzer, checked for invariants: one column per
//! select item, a `rewrite` that `strip` undoes, and, with `SQLTY_PG_DSN`, the column
//! names and types PostgreSQL gives, never NULL in a column typed not to hold it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use postgres::types::{FromSql, Type};
use proptest::prelude::*;
use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{Expr, ExprAttribute, ExprName};
use sqlty_sql::analyzer::{self, Column};

use crate::draw::{check, Draw};
use crate::engine;

/// Column types, and a literal of each for COALESCE and VALUES.
const TYPES: [(&str, &str); 18] = [
    ("BIGINT", "2::BIGINT"),
    ("BOOLEAN", "TRUE"),
    ("BYTEA", "'\\x00'::BYTEA"),
    ("DATE", "DATE '2024-01-01'"),
    ("DOUBLE PRECISION", "2.5::DOUBLE PRECISION"),
    ("INTEGER", "1"),
    ("INTEGER[]", "ARRAY[1]"),
    ("INTERVAL", "INTERVAL '1 day'"),
    ("JSONB", "'{}'::JSONB"),
    ("NUMERIC(10, 2)", "1.5"),
    ("REAL", "1.5::REAL"),
    ("SMALLINT", "3::SMALLINT"),
    ("TEXT", "'a'"),
    ("TEXT[]", "ARRAY['a']"),
    ("TIMESTAMP", "TIMESTAMP '2024-01-01 00:00:00'"),
    ("TIMESTAMPTZ", "now()"),
    ("UUID", "gen_random_uuid()"),
    ("VARCHAR(20)", "'b'::VARCHAR(20)"),
];
const NUMERIC: [&str; 6] = [
    "INTEGER",
    "BIGINT",
    "SMALLINT",
    "NUMERIC(10, 2)",
    "DOUBLE PRECISION",
    "REAL",
];
const TEXT: [&str; 2] = ["TEXT", "VARCHAR(20)"];
const TEMPORAL: [&str; 4] = ["DATE", "TIMESTAMP", "TIMESTAMPTZ", "INTERVAL"];

fn literal(kind: &str) -> &'static str {
    TYPES
        .iter()
        .find(|(name, _)| *name == kind)
        .map_or("NULL", |(_, value)| value)
}

#[derive(Debug, Clone)]
struct Col {
    name: String,
    kind: &'static str,
    not_null: bool,
}

#[derive(Debug)]
struct Table {
    name: String,
    columns: Vec<Col>,
}

impl Table {
    fn ddl(&self) -> String {
        let columns: Vec<String> = self
            .columns
            .iter()
            .map(|c| {
                format!(
                    "{} {}{}",
                    c.name,
                    c.kind,
                    if c.not_null { " NOT NULL" } else { "" }
                )
            })
            .collect();
        format!(
            "CREATE TABLE {} (id INTEGER PRIMARY KEY, {});",
            self.name,
            columns.join(", ")
        )
    }
}

#[derive(Debug)]
struct Case {
    tables: Vec<Table>,
    query: String,
    items: usize,
}

impl Case {
    fn schema(&self) -> String {
        self.tables
            .iter()
            .map(Table::ddl)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn tables(d: &mut Draw<'_>) -> Vec<Table> {
    (0..d.int(1, 2))
        .map(|i| Table {
            name: format!("t{i}"),
            columns: (0..d.int(1, 4))
                .map(|j| Col {
                    name: format!("c{j}"),
                    kind: d.pick(&TYPES).0,
                    not_null: d.bool(),
                })
                .collect(),
        })
        .collect()
}

fn expression(d: &mut Draw<'_>, columns: &[(&str, Col)], aggregate: bool) -> String {
    let (alias, column) = d.pick(columns);
    let reference = format!("{alias}.{}", column.name);
    let kind = column.kind;
    let mut choices = vec!["column", "cast_text", "coalesce", "is_null", "case"];
    if NUMERIC.contains(&kind) {
        choices.extend(["arith", "abs"]);
    }
    if TEXT.contains(&kind) {
        choices.extend(["lower", "concat"]);
    }
    if aggregate {
        choices = vec!["count", "count_star"];
        if NUMERIC.contains(&kind) || TEXT.contains(&kind) || TEMPORAL.contains(&kind) {
            choices.extend(["max", "min"]);
        }
        if NUMERIC.contains(&kind) {
            choices.extend(["sum", "avg"]);
        }
    }
    match d.pick(&choices) {
        "column" => reference,
        "cast_text" => format!("{reference}::TEXT"),
        "coalesce" => format!("COALESCE({reference}, {})", literal(kind)),
        "is_null" => format!("{reference} IS NULL"),
        "case" => format!("CASE WHEN {reference} IS NOT NULL THEN {reference} END"),
        "arith" => format!("{reference} + 1"),
        "abs" => format!("abs({reference})"),
        "lower" => format!("lower({reference})"),
        "concat" => format!("{reference} || 'x'"),
        "count" => format!("count({reference})"),
        "count_star" => "count(*)".into(),
        name => format!("{name}({reference})"),
    }
}

fn cases(d: &mut Draw<'_>) -> Case {
    let tables = tables(d);
    let mut sources = vec![("a", &tables[0])];
    let mut from = format!("{} a", tables[0].name);
    if let Some(right) = tables.get(1) {
        match d.pick(&["JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN", ","]) {
            "," => from += &format!(", {} b", right.name),
            join => from += &format!(" {join} {} b ON b.id = a.id", right.name),
        }
        sources.push(("b", right));
    }
    let columns: Vec<(&str, Col)> = sources
        .iter()
        .flat_map(|(alias, table)| table.columns.iter().map(|column| (*alias, column.clone())))
        .collect();
    let aggregate = d.bool();
    let items = d.list(1, 4, |d| expression(d, &columns, aggregate));
    let select: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(i, item)| format!("{item} AS x{i}"))
        .collect();
    let mut query = format!("SELECT {} FROM {from}", select.join(", "));
    if d.bool() {
        let (alias, column) = d.pick(&columns);
        query += &format!(" WHERE {alias}.{} IS NOT NULL", column.name);
    }
    if d.bool() && !aggregate {
        query = format!("{query} UNION ALL {query}");
    }
    let items = items.len();
    Case {
        tables,
        query,
        items,
    }
}

#[test]
fn queries_analyze_with_one_column_per_item() {
    check(cases, |case| {
        let schema = engine::schema(&case.schema(), "postgres");
        let columns = engine::analyze_select(&case.query, "postgres", Some(&schema));
        prop_assert!(columns.is_some(), "not analyzed");
        let names: Vec<String> = columns
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.name)
            .collect();
        let wanted: Vec<String> = (0..case.items).map(|i| format!("x{i}")).collect();
        prop_assert_eq!(names, wanted);
        Ok(())
    });
}

#[test]
fn rewrite_and_strip_round_trip() {
    check(cases, |case| {
        let schema = engine::schema(&case.schema(), "postgres");
        let source = format!("q = \"--sql\\n{}\"\n", case.query);
        let rewritten = engine::rewrite(&source, &schema, None);
        prop_assert_eq!(&engine::strip(&rewritten, None), &source);
        prop_assert_eq!(&engine::rewrite(&rewritten, &schema, None), &rewritten);
        let stale: Vec<_> = engine::check(&rewritten, "postgres", &schema, None)
            .into_iter()
            .filter(|(rule, _)| matches!(*rule, "stale-type" | "narrow-type"))
            .collect();
        prop_assert!(stale.is_empty(), "{:?}\n{}", stale, rewritten);
        Ok(())
    });
}

/// sqlty SQL type -> `pg_type.typname`.
const SQL_TO_PG: [(&str, &str); 29] = [
    ("SMALLINT", "int2"),
    ("INT", "int4"),
    ("INTEGER", "int4"),
    ("BIGINT", "int8"),
    ("DECIMAL", "numeric"),
    ("NUMERIC", "numeric"),
    ("FLOAT", "float4"),
    ("REAL", "float4"),
    ("DOUBLE", "float8"),
    ("TEXT", "text"),
    ("VARCHAR", "varchar"),
    ("CHAR", "bpchar"),
    ("BOOLEAN", "bool"),
    ("DATE", "date"),
    ("TIME", "time"),
    ("TIMETZ", "timetz"),
    ("TIMESTAMP", "timestamp"),
    ("TIMESTAMPTZ", "timestamptz"),
    ("INTERVAL", "interval"),
    ("UUID", "uuid"),
    ("JSON", "json"),
    ("JSONB", "jsonb"),
    ("BYTEA", "bytea"),
    ("VARBINARY", "bytea"),
    ("INET", "inet"),
    ("CIDR", "cidr"),
    ("MACADDR", "macaddr"),
    ("MONEY", "money"),
    ("VOID", "void"),
];

/// Python type -> server types a column typed only by it (a literal) may have.
const PYTHON_TO_PG: [(&str, &[&str]); 9] = [
    ("int", &["int2", "int4", "int8"]),
    ("str", &["text", "varchar", "bpchar", "unknown", "name"]),
    ("bool", &["bool"]),
    ("float", &["float4", "float8"]),
    ("decimal.Decimal", &["numeric"]),
    ("datetime.date", &["date"]),
    ("datetime.datetime", &["timestamp", "timestamptz"]),
    ("datetime.time", &["time", "timetz"]),
    ("datetime.timedelta", &["interval"]),
];

/// `DECIMAL(10, 2)` -> `numeric`, `ARRAY<INT>` and `ARRAY<ARRAY<INT>>` -> `int4[]`.
fn normalise(sql_type: &str) -> String {
    let upper = sql_type.trim().to_uppercase();
    if let Some(inner) = upper
        .strip_prefix("ARRAY<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return format!("{}[]", normalise(inner).trim_end_matches("[]"));
    }
    let base = upper
        .split(['(', '<', '['])
        .next()
        .unwrap_or_default()
        .trim();
    SQL_TO_PG
        .iter()
        .find(|(name, _)| *name == base)
        .map_or_else(|| base.to_lowercase(), |(_, pg)| (*pg).to_string())
}

/// Why sqlty's column differs from the server's, or `None` when they agree.
fn compare(ours: &Column, name: &str, server_type: &str, server_kind: &str) -> Option<String> {
    if ours.name != name {
        return Some(format!("name {:?}, server says {name:?}", ours.name));
    }
    if ["d", "e", "c"].contains(&server_kind) {
        return None;
    }
    if let Some(sql_type) = &ours.sql_type {
        let expected = normalise(sql_type);
        return (expected != server_type).then(|| {
            format!("column {name:?}: type {sql_type} ({expected}), server says {server_type}")
        });
    }
    let display = ours.display_type().unwrap_or_default();
    let python_type = display.strip_suffix(" | None").unwrap_or(&display);
    let (_, families) = PYTHON_TO_PG
        .iter()
        .find(|(python, _)| *python == python_type)?;
    (!families.contains(&server_type))
        .then(|| format!("column {name:?}: type {python_type}, server says {server_type}"))
}

/// Any value, read only for whether it is NULL.
struct Null(bool);

impl<'a> FromSql<'a> for Null {
    fn from_sql(_: &Type, _: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(false))
    }

    fn from_sql_null(_: &Type) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(true))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

struct Server {
    client: postgres::Client,
    /// `oid -> (name, kind)`; an array's name is its element's with `[]`.
    types: HashMap<u32, (String, String)>,
}

static SERVER: LazyLock<Option<Mutex<Server>>> = LazyLock::new(|| {
    let dsn = std::env::var("SQLTY_PG_DSN").ok()?;
    let mut client =
        postgres::Client::connect(&dsn, postgres::NoTls).expect("connects to SQLTY_PG_DSN");
    let rows = client
        .query(
            "SELECT oid, typname::text, typtype::text, typelem, typcategory::text FROM pg_type",
            &[],
        )
        .expect("reads pg_type");
    let by_oid: HashMap<u32, (String, String, u32, String)> = rows
        .iter()
        .map(|row| (row.get(0), (row.get(1), row.get(2), row.get(3), row.get(4))))
        .collect();
    let types = by_oid
        .iter()
        .map(
            |(oid, (name, kind, elem, category))| match by_oid.get(elem) {
                Some((element, element_kind, _, _)) if category == "A" => {
                    (*oid, (format!("{element}[]"), element_kind.clone()))
                }
                _ => (*oid, (name.clone(), kind.clone())),
            },
        )
        .collect();
    Some(Mutex::new(Server { client, types }))
});

/// The server prepares each query and runs it over 0 to 2 rows whose nullable columns are
/// all NULL: it must agree on names and types, and return NULL only where sqlty allows it.
#[test]
fn server_agrees() {
    let Some(server) = SERVER.as_ref() else {
        eprintln!("server_agrees: set SQLTY_PG_DSN to compare with a server");
        return;
    };
    check(
        |d| (cases(d), d.int(0, 2)),
        |(case, rows)| {
            let schema = engine::schema(&case.schema(), "postgres");
            let ours = engine::analyze_select(&case.query, "postgres", Some(&schema));
            prop_assert!(ours.is_some(), "not analyzed");
            let ours = ours.unwrap_or_default();
            let mut server = server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Server { client, types } = &mut *server;
            for table in &case.tables {
                let mut load = format!(
                    "DROP TABLE IF EXISTS {} CASCADE; {}",
                    table.name,
                    table.ddl()
                );
                for i in 0..rows {
                    let values: Vec<&str> = table
                        .columns
                        .iter()
                        .map(|c| if c.not_null { literal(c.kind) } else { "NULL" })
                        .collect();
                    load += &format!(
                        "INSERT INTO {} VALUES ({i}, {});",
                        table.name,
                        values.join(", ")
                    );
                }
                client.batch_execute(&load).expect("loads the tables");
            }
            // A combination the server rejects (say `max` of an array) says nothing about sqlty.
            let Ok(statement) = client.prepare(&case.query) else {
                return Err(TestCaseError::reject("the server rejects the query"));
            };
            let Ok(rows) = client.query(&statement, &[]) else {
                return Err(TestCaseError::reject("the server fails the query"));
            };
            let mut problems = Vec::new();
            prop_assert_eq!(ours.len(), statement.columns().len());
            for (column, described) in ours.iter().zip(statement.columns()) {
                let (server_type, kind) = types
                    .get(&described.type_().oid())
                    .cloned()
                    .unwrap_or_else(|| (format!("oid {}", described.type_().oid()), "b".into()));
                problems.extend(compare(column, described.name(), &server_type, &kind));
            }
            for row in &rows {
                for (i, column) in ours.iter().enumerate() {
                    let python_type = column.display_type().unwrap_or_else(|| "typing.Any".into());
                    if row.get::<_, Null>(i).0
                        && !python_type.ends_with(" | None")
                        && python_type != "typing.Any"
                    {
                        problems.push(format!(
                            "column {:?} typed {python_type} holds NULL",
                            column.name
                        ));
                    }
                }
            }
            prop_assert!(
                problems.is_empty(),
                "{}\n{}\n  {}",
                case.schema(),
                case.query,
                problems.join("\n  ")
            );
            Ok(())
        },
    );
}

/// Type names by the grammar sqlparser reads, with and without parameters.
const SCALAR_TYPES: [&str; 58] = [
    "INT",
    "INTEGER",
    "SMALLINT",
    "BIGINT",
    "REAL",
    "FLOAT",
    "FLOAT(8)",
    "DOUBLE PRECISION",
    "NUMERIC",
    "NUMERIC(10)",
    "NUMERIC(10, 2)",
    "DECIMAL(5, 1)",
    "DEC",
    "DECFLOAT",
    "CHAR",
    "CHAR(3)",
    "CHARACTER VARYING(9)",
    "VARCHAR(20)",
    "NVARCHAR(4)",
    "TEXT",
    "CLOB",
    "CHARACTER LARGE OBJECT",
    "BINARY LARGE OBJECT",
    "BLOB",
    "BYTEA",
    "BINARY(4)",
    "VARBINARY(8)",
    "BOOLEAN",
    "BOOL",
    "DATE",
    "TIME",
    "TIME(3)",
    "TIME WITH TIME ZONE",
    "TIMESTAMP",
    "TIMESTAMP(6) WITH TIME ZONE",
    "TIMESTAMP WITHOUT TIME ZONE",
    "TIMESTAMPTZ",
    "INTERVAL",
    "INTERVAL DAY TO SECOND",
    "INTERVAL YEAR TO MONTH",
    "INTERVAL HOUR",
    "JSON",
    "JSONB",
    "UUID",
    "BIT(3)",
    "BIT VARYING(5)",
    "VARBIT",
    "INET",
    "CIDR",
    "MACADDR",
    "MONEY",
    "XML",
    "INT4RANGE",
    "TSTZRANGE",
    "POINT",
    "OID",
    "REGCLASS",
    "TSVECTOR",
];
const DIALECT_TYPES: [(&str, &[&str]); 4] = [
    (
        "duckdb",
        &[
            "HUGEINT",
            "UBIGINT",
            "UTINYINT",
            "STRUCT(a INT, b VARCHAR)",
            "MAP(VARCHAR, INT)",
            "UNION(n INT, s VARCHAR)",
        ],
    ),
    (
        "mysql",
        &[
            "SIGNED",
            "UNSIGNED",
            "INT UNSIGNED",
            "YEAR",
            "DATETIME",
            "DATETIME(6)",
            "TINYTEXT",
            "MEDIUMBLOB",
            "ENUM('a', 'b')",
            "SET('a', 'b')",
            "TINYINT(1)",
        ],
    ),
    ("postgres", &[]),
    (
        "sqlite",
        &[
            "UNSIGNED_BIG_INT",
            "NATIVE_CHARACTER",
            "VARYING_CHARACTER",
            "ANY",
            "WIDGET",
        ],
    ),
];

fn type_name(d: &mut Draw<'_>, own: &[&str]) -> String {
    let scalars: Vec<&str> = SCALAR_TYPES.iter().chain(own).copied().collect();
    let mut name = d.pick(&scalars).to_string();
    for _ in 0..d.int(0, 2) {
        name = match d.pick(&["[]", "[3]", " ARRAY", " MULTISET", "row"]) {
            "row" => format!("ROW(a {name}, b {})", d.pick(&scalars)),
            wrap => format!("{name}{wrap}"),
        };
    }
    name
}

/// The dotted names a type expression uses: `typing.Any`, `list`, ...
fn dotted(expr: &Expr) -> String {
    match expr {
        Expr::Attribute(ExprAttribute { value, attr, .. }) => format!("{}.{attr}", dotted(value)),
        Expr::Name(ExprName { id, .. }) => id.to_string(),
        _ => "?".into(),
    }
}

#[derive(Default)]
struct Names(Vec<String>);

impl Visitor<'_> for Names {
    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Attribute(_) => self.0.push(dotted(expr)),
            Expr::Name(ExprName { id, .. }) => self.0.push(id.to_string()),
            _ => walk_expr(self, expr),
        }
    }
}

#[test]
fn type_names_map_to_valid_python_types() {
    let mut allowed: Vec<&str> = analyzer::python_types().into_iter().collect();
    allowed.extend(["list", "dict", "tuple", "typing.Literal", "None"]);
    check(
        |d| {
            let (dialect, own) = d.pick(&DIALECT_TYPES);
            (dialect, type_name(d, own))
        },
        |(dialect, name)| {
            let query = format!("SELECT CAST(x AS {name}) AS c");
            if analyzer::parse_error(&query, dialect).is_some() {
                return Ok(());
            }
            let columns = engine::analyze_select(&query, dialect, None);
            prop_assert!(
                columns.is_some(),
                "{}: {} parses but is not analyzed",
                dialect,
                query
            );
            let Some(python_type) = columns.unwrap_or_default().remove(0).display_type() else {
                return Ok(());
            };
            let parsed = ruff_python_parser::parse_expression(&python_type);
            prop_assert!(
                parsed.is_ok(),
                "{}: {} -> {} does not parse",
                dialect,
                name,
                python_type
            );
            let mut names = Names::default();
            if let Ok(parsed) = parsed {
                names.visit_expr(parsed.expr());
            }
            let unknown: Vec<String> = names
                .0
                .into_iter()
                .filter(|name| !allowed.contains(&name.as_str()))
                .collect();
            prop_assert!(
                unknown.is_empty(),
                "{}: {} -> {} uses {:?}",
                dialect,
                name,
                python_type,
                unknown
            );
            Ok(())
        },
    );
}
