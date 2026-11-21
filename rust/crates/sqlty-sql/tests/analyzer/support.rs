//! Helpers shared by the analyzer tests: render what a query infers as text.
use std::fmt::Write as _;

use sqlty_sql::analyzer::{analyze_with_diagnostics, load_schema_with_skipped, Schema};

pub fn load_schema(ddl: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(ddl, dialect).0
}

/// One query against an optional schema: its columns (`name: python | sql | nullability`),
/// then its diagnostics, or the error / "no rows" outcome.
pub fn report(sql: &str, dialect: &str, ddl: Option<&str>) -> String {
    let schema = ddl.map(|ddl| load_schema(ddl, dialect));
    let (result, diagnostics) = analyze_with_diagnostics(sql, dialect, schema.as_deref());
    let mut out = String::new();
    match result {
        None => out.push_str("no rows\n"),
        Some(columns) => {
            for column in columns {
                writeln!(
                    out,
                    "{}: {} | {} | {:?}",
                    column.name,
                    column.python_type.as_deref().unwrap_or("?"),
                    column.sql_type.as_deref().unwrap_or("?"),
                    column.nullability,
                )
                .unwrap();
            }
        }
    }
    for diagnostic in diagnostics {
        writeln!(out, "[{}] {}", diagnostic.rule.name(), diagnostic.message).unwrap();
    }
    out
}

/// [`report`] for several queries, each headed by its SQL.
pub fn reports(queries: &[&str], dialect: &str, ddl: Option<&str>) -> String {
    queries
        .iter()
        .map(|sql| format!("{sql}\n{}", report(sql, dialect, ddl)))
        .collect::<Vec<_>>()
        .join("\n")
}
