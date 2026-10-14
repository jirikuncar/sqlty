//! Helpers shared by the analyzer's unit tests: render what a query infers as text, so a
//! test can snapshot the columns and diagnostics of several queries at once.

use std::fmt::Write as _;

use sqlparser::ast::{Expr, Statement};
use sqlparser::parser::Parser;

use super::preprocess::parse_first;
use super::{analyze_with_diagnostics, load_schema, SqlDialect};

/// One query against an optional schema: its columns (`name: python | sql | nullability`),
/// then its diagnostics, or the error / "no rows" outcome.
pub(super) fn report(sql: &str, dialect: &str, ddl: Option<&str>) -> String {
    let schema = ddl.map(|ddl| load_schema(ddl, dialect));
    let (result, diagnostics) = analyze_with_diagnostics(sql, dialect, schema.as_deref());
    let mut out = String::new();
    match result {
        Err(error) => writeln!(out, "error: {error}").unwrap(),
        Ok(None) => out.push_str("no rows\n"),
        Ok(Some(columns)) => {
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
pub(super) fn reports(queries: &[&str], dialect: &str, ddl: Option<&str>) -> String {
    queries
        .iter()
        .map(|sql| format!("{sql}\n{}", report(sql, dialect, ddl)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The first statement of `sql`.
pub(super) fn statement(sql: &str, dialect: SqlDialect) -> Statement {
    parse_first(sql, dialect).expect("test SQL parses")
}

/// A scalar expression, as PostgreSQL parses it.
pub(super) fn expr(sql: &str) -> Expr {
    let dialect = SqlDialect::Postgres.parser_dialect();
    Parser::new(dialect.as_ref())
        .try_with_sql(sql)
        .and_then(|mut parser| parser.parse_expr())
        .expect("test expression parses")
}
