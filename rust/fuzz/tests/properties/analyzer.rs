//! Random schemas and queries through the analyzer, checked for invariants: one column per
//! select item, a `rewrite` that `strip` undoes, and, with `SQLTY_PG_DSN`, the column
//! names and types PostgreSQL gives, never NULL in a column typed not to hold it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use postgres::SimpleQueryMessage;
use proptest::prelude::*;
use regex::Regex;
use sqlty_sql::analyzer::{self, Column};

use crate::draw::{check, Draw};
use crate::engine;

/// Column types, and a literal of each for COALESCE and VALUES.
#[rustfmt::skip]
const TYPES: [(&str, &str); 18] = [
    ("BIGINT", "2::BIGINT"), ("BOOLEAN", "TRUE"), ("BYTEA", "'\\x00'::BYTEA"),
    ("DATE", "DATE '2024-01-01'"), ("DOUBLE PRECISION", "2.5::DOUBLE PRECISION"),
    ("INTEGER", "1"), ("INTEGER[]", "ARRAY[1]"), ("INTERVAL", "INTERVAL '1 day'"),
    ("JSONB", "'{}'::JSONB"), ("NUMERIC(10, 2)", "1.5"), ("REAL", "1.5::REAL"),
    ("SMALLINT", "3::SMALLINT"), ("TEXT", "'a'"), ("TEXT[]", "ARRAY['a']"),
    ("TIMESTAMP", "TIMESTAMP '2024-01-01 00:00:00'"), ("TIMESTAMPTZ", "now()"),
    ("UUID", "gen_random_uuid()"), ("VARCHAR(20)", "'b'::VARCHAR(20)"),
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

/// `(name, columns)`, each column `(name, type, not null)`.
type Table = (String, Vec<(String, &'static str, bool)>);

#[derive(Debug)]
struct Case {
    tables: Vec<Table>,
    query: String,
    items: usize,
}

fn ddl((name, columns): &Table) -> String {
    let columns: Vec<String> = columns
        .iter()
        .map(|(c, kind, not_null)| {
            format!("{c} {kind}{}", if *not_null { " NOT NULL" } else { "" })
        })
        .collect();
    format!(
        "CREATE TABLE {name} (id INTEGER PRIMARY KEY, {});",
        columns.join(", ")
    )
}

impl Case {
    fn schema(&self) -> String {
        self.tables.iter().map(ddl).collect::<Vec<_>>().join("\n")
    }
}

fn expression(d: &mut Draw<'_>, columns: &[(&str, String, &str)], aggregate: bool) -> String {
    let (alias, name, kind) = d.pick(columns);
    let column = format!("{alias}.{name}");
    let mut choices = if aggregate {
        vec!["count", "count_star"]
    } else {
        vec!["column", "cast_text", "coalesce", "is_null", "case"]
    };
    let (numeric, text) = (NUMERIC.contains(&kind), TEXT.contains(&kind));
    match aggregate {
        false if numeric => choices.extend(["arith", "abs"]),
        false if text => choices.extend(["lower", "concat"]),
        true if numeric || text || TEMPORAL.contains(&kind) => choices.extend(["max", "min"]),
        _ => {}
    }
    if aggregate && numeric {
        choices.extend(["sum", "avg"]);
    }
    match d.pick(&choices) {
        "column" => column,
        "cast_text" => format!("{column}::TEXT"),
        "coalesce" => format!("COALESCE({column}, {})", literal(kind)),
        "is_null" => format!("{column} IS NULL"),
        "case" => format!("CASE WHEN {column} IS NOT NULL THEN {column} END"),
        "arith" => format!("{column} + 1"),
        "concat" => format!("{column} || 'x'"),
        "count_star" => "count(*)".into(),
        function => format!("{function}({column})"),
    }
}

fn cases(d: &mut Draw<'_>) -> Case {
    let tables: Vec<Table> = (0..d.int(1, 2))
        .map(|i| {
            (
                format!("t{i}"),
                (0..d.int(1, 4))
                    .map(|j| (format!("c{j}"), d.pick(&TYPES).0, d.bool()))
                    .collect(),
            )
        })
        .collect();
    let mut from = format!("{} a", tables[0].0);
    if let Some((right, _)) = tables.get(1) {
        from += &match d.pick(&["JOIN", "LEFT JOIN", "RIGHT JOIN", "FULL JOIN", ","]) {
            "," => format!(", {right} b"),
            join => format!(" {join} {right} b ON b.id = a.id"),
        };
    }
    let columns: Vec<(&str, String, &str)> = tables
        .iter()
        .zip(["a", "b"])
        .flat_map(|((_, columns), alias)| {
            columns
                .iter()
                .map(move |(name, kind, _)| (alias, name.clone(), *kind))
        })
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
        let (alias, name, _) = d.pick(&columns);
        query += &format!(" WHERE {alias}.{name} IS NOT NULL");
    }
    if d.bool() && !aggregate {
        query = format!("{query} UNION ALL {query}");
    }
    Case {
        tables,
        query,
        items: items.len(),
    }
}

fn analyzed(case: &Case) -> Result<(analyzer::Schema, Vec<Column>), TestCaseError> {
    let schema = engine::schema(&case.schema(), "postgres");
    let columns = engine::analyze_select(&case.query, "postgres", Some(&schema));
    prop_assert!(columns.is_some(), "not analyzed");
    Ok((schema, columns.unwrap_or_default()))
}

#[test]
fn queries_analyze_with_one_column_per_item() {
    check(cases, |case| {
        let names: Vec<String> = analyzed(&case)?.1.into_iter().map(|c| c.name).collect();
        prop_assert_eq!(
            names,
            (0..case.items).map(|i| format!("x{i}")).collect::<Vec<_>>()
        );
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
        let found = engine::check(&rewritten, "postgres", &schema, None);
        let stale: Vec<_> = found
            .iter()
            .filter(|(rule, _)| matches!(*rule, "stale-type" | "narrow-type"))
            .collect();
        prop_assert!(stale.is_empty(), "{:?}\n{}", stale, rewritten);
        Ok(())
    });
}

/// sqlty SQL type -> `pg_type.typname`.
#[rustfmt::skip]
const SQL_TO_PG: [(&str, &str); 29] = [
    ("SMALLINT", "int2"), ("INT", "int4"), ("INTEGER", "int4"), ("BIGINT", "int8"),
    ("DECIMAL", "numeric"), ("NUMERIC", "numeric"), ("FLOAT", "float4"), ("REAL", "float4"),
    ("DOUBLE", "float8"), ("TEXT", "text"), ("VARCHAR", "varchar"), ("CHAR", "bpchar"),
    ("BOOLEAN", "bool"), ("DATE", "date"), ("TIME", "time"), ("TIMETZ", "timetz"),
    ("TIMESTAMP", "timestamp"), ("TIMESTAMPTZ", "timestamptz"), ("INTERVAL", "interval"),
    ("UUID", "uuid"), ("JSON", "json"), ("JSONB", "jsonb"), ("BYTEA", "bytea"),
    ("VARBINARY", "bytea"), ("INET", "inet"), ("CIDR", "cidr"), ("MACADDR", "macaddr"),
    ("MONEY", "money"), ("VOID", "void"),
];

/// Python type -> server types a column typed only by it (a literal) may have.
#[rustfmt::skip]
const PYTHON_TO_PG: [(&str, &[&str]); 9] = [
    ("int", &["int2", "int4", "int8"]), ("str", &["text", "varchar", "bpchar", "unknown", "name"]),
    ("bool", &["bool"]), ("float", &["float4", "float8"]), ("decimal.Decimal", &["numeric"]),
    ("datetime.date", &["date"]), ("datetime.datetime", &["timestamp", "timestamptz"]),
    ("datetime.time", &["time", "timetz"]), ("datetime.timedelta", &["interval"]),
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
        .map_or_else(|| base.to_lowercase(), |(_, pg)| (*pg).into())
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

/// The client, and `oid -> (name, kind)` of every type; an array's is its element's with `[]`.
type Server = (postgres::Client, HashMap<u32, (String, String)>);

static SERVER: LazyLock<Option<Mutex<Server>>> = LazyLock::new(|| {
    let dsn = std::env::var("SQLTY_PG_DSN").ok()?;
    let mut client =
        postgres::Client::connect(&dsn, postgres::NoTls).expect("connects to SQLTY_PG_DSN");
    let types = client
        .query(
            "SELECT t.oid, coalesce(e.typname || '[]', t.typname::text), coalesce(e.typtype, t.typtype)::text
             FROM pg_type t LEFT JOIN pg_type e ON t.typcategory = 'A' AND e.oid = t.typelem",
            &[],
        )
        .expect("reads pg_type");
    let types = types
        .iter()
        .map(|row| (row.get(0), (row.get(1), row.get(2))))
        .collect();
    Some(Mutex::new((client, types)))
});

/// The server prepares each query and runs it over 0 to 2 rows whose nullable columns are
/// all NULL: it must agree on names and types, and return NULL only where sqlty allows it.
#[test]
fn server_agrees() {
    let Some(server) = SERVER.as_ref() else {
        return eprintln!("server_agrees: set SQLTY_PG_DSN to compare with a server");
    };
    check(
        |d| (cases(d), d.int(0, 2)),
        |(case, rows)| {
            let ours = analyzed(&case)?.1;
            let (client, types) = &mut *server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (name, columns) in &case.tables {
                let values: Vec<&str> = columns
                    .iter()
                    .map(|(_, kind, not_null)| if *not_null { literal(kind) } else { "NULL" })
                    .collect();
                let rows: String = (0..rows)
                    .map(|i| format!("INSERT INTO {name} VALUES ({i}, {});", values.join(", ")))
                    .collect();
                let table = ddl(&(name.clone(), columns.clone()));
                client
                    .batch_execute(&format!(
                        "DROP TABLE IF EXISTS {name} CASCADE; {table} {rows}"
                    ))
                    .expect("loads the tables");
            }
            // A combination the server rejects (say `max` of an array) says nothing about sqlty.
            let (Ok(statement), Ok(results)) = (
                client.prepare(&case.query),
                client.simple_query(&case.query),
            ) else {
                return Err(TestCaseError::reject("the server rejects the query"));
            };
            prop_assert_eq!(ours.len(), statement.columns().len());
            let mut problems: Vec<String> = ours
                .iter()
                .zip(statement.columns())
                .filter_map(|(column, described)| {
                    let oid = described.type_().oid();
                    let (server_type, kind) = types
                        .get(&oid)
                        .cloned()
                        .unwrap_or_else(|| (format!("oid {oid}"), "b".into()));
                    compare(column, described.name(), &server_type, &kind)
                })
                .collect();
            for result in &results {
                let SimpleQueryMessage::Row(row) = result else {
                    continue;
                };
                for (i, column) in ours.iter().enumerate() {
                    let python_type = column.display_type().unwrap_or_else(|| "typing.Any".into());
                    if row.get(i).is_none()
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
const SCALAR_TYPES: &str = "INT|INTEGER|SMALLINT|BIGINT|REAL|FLOAT|FLOAT(8)|DOUBLE PRECISION|NUMERIC|NUMERIC(10)\
    |NUMERIC(10, 2)|DECIMAL(5, 1)|DEC|DECFLOAT|CHAR|CHAR(3)|CHARACTER VARYING(9)|VARCHAR(20)|NVARCHAR(4)|TEXT|CLOB\
    |CHARACTER LARGE OBJECT|BINARY LARGE OBJECT|BLOB|BYTEA|BINARY(4)|VARBINARY(8)|BOOLEAN|BOOL|DATE|TIME|TIME(3)\
    |TIME WITH TIME ZONE|TIMESTAMP|TIMESTAMP(6) WITH TIME ZONE|TIMESTAMP WITHOUT TIME ZONE|TIMESTAMPTZ|INTERVAL\
    |INTERVAL DAY TO SECOND|INTERVAL YEAR TO MONTH|INTERVAL HOUR|JSON|JSONB|UUID|BIT(3)|BIT VARYING(5)|VARBIT|INET\
    |CIDR|MACADDR|MONEY|XML|INT4RANGE|TSTZRANGE|POINT|OID|REGCLASS|TSVECTOR";
#[rustfmt::skip]
const DIALECT_TYPES: [(&str, &str); 4] = [
    ("duckdb", "HUGEINT|UBIGINT|UTINYINT|STRUCT(a INT, b VARCHAR)|MAP(VARCHAR, INT)|UNION(n INT, s VARCHAR)"),
    ("mysql", "SIGNED|UNSIGNED|INT UNSIGNED|YEAR|DATETIME|DATETIME(6)|TINYTEXT|MEDIUMBLOB|ENUM('a', 'b')|SET('a', 'b')|TINYINT(1)"),
    ("postgres", ""),
    ("sqlite", "UNSIGNED_BIG_INT|NATIVE_CHARACTER|VARYING_CHARACTER|ANY|WIDGET"),
];

fn type_name(d: &mut Draw<'_>, own: &str) -> String {
    let scalars: Vec<&str> = SCALAR_TYPES
        .split('|')
        .chain(own.split('|').filter(|t| !t.is_empty()))
        .collect();
    let mut name = d.pick(&scalars).to_string();
    for _ in 0..d.int(0, 2) {
        name = match d.pick(&["[]", "[3]", " ARRAY", " MULTISET", "row"]) {
            "row" => format!("ROW(a {name}, b {})", d.pick(&scalars)),
            wrap => format!("{name}{wrap}"),
        };
    }
    name
}

/// String literals, and the dotted names a type expression uses: `typing.Any`, `list`, ...
static TOKENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"'[^']*'|"[^"]*"|[A-Za-z_][\w.]*"#).expect("a regex"));

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
            let parses = ruff_python_parser::parse_expression(&python_type).is_ok();
            prop_assert!(
                parses,
                "{}: {} -> {} does not parse",
                dialect,
                name,
                python_type
            );
            let unknown: Vec<&str> = TOKENS
                .find_iter(&python_type)
                .map(|token| token.as_str())
                .filter(|token| !token.starts_with(['\'', '"']) && !allowed.contains(token))
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
