//! Helpers shared by the analyzer's unit tests of private AST helpers.

use sqlparser::ast::{Expr, Statement};
use sqlparser::parser::Parser;

use super::preprocess::parse_first;
use super::SqlDialect;

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
