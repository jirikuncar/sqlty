//! The analyzer's public entry points: type inference, column analysis, parse checks.

use std::sync::Arc;

use sqlparser::parser::Parser;

use crate::diagnostics::{Diagnostics, NameDiagnostic};

use super::dialect::SqlDialect;
use super::dml::statement_returns_rows;
use super::expr::from_sql_type;
use super::names::alias_name;
use super::preprocess::{parse_first, parse_query, parser_input};
use super::query::{Analyzer, Column};
use super::{Inferred, Schema, SchemaData, SkippedObject, ANY};

/// Load schema from CREATE TABLE, VIEW, DOMAIN and TYPE ... AS ENUM statements; in
/// PostgreSQL also ALTER, DROP, search_path and functions (see [`crate::catalog`]).
/// Returns the `(kind, name)` of the tables that could not be parsed even without
/// their constraints and of the views that could not be parsed.
pub fn load_schema_with_skipped(schema_sql: &str, dialect: &str) -> (Schema, Vec<SkippedObject>) {
    let (catalog, skipped) = crate::catalog::load(schema_sql, dialect);
    (Arc::new(catalog), skipped)
}

#[cfg(test)]
pub(crate) fn load_schema(schema_sql: &str, dialect: &str) -> Schema {
    load_schema_with_skipped(schema_sql, dialect).0
}

/// Output columns of `view`'s query, named by its column list, as read against `catalog`.
pub(crate) fn view_columns(
    catalog: &SchemaData,
    dialect: SqlDialect,
    view: &sqlparser::ast::CreateView,
) -> Vec<Column> {
    let analyzer = Analyzer::new(catalog, dialect);
    let mut columns = match analyzer.query_columns(&view.query) {
        Ok(Some(columns)) => columns,
        _ => vec![Column::unknown("*")],
    };
    for (column, name) in columns.iter_mut().zip(&view.columns) {
        column.name = alias_name(&name.name, dialect);
    }
    columns
}

/// `(python_type, sql_type)` of a column of SQL type `sql_type`, whose Python type is
/// `python_type` when that is not the one the SQL type maps to.
pub(crate) fn typed(
    sql_type: String,
    python_type: Option<String>,
    dialect: SqlDialect,
) -> (Option<String>, Option<String>) {
    match python_type {
        Some(python_type) => (Some(python_type), Some(sql_type)),
        None => from_sql_type(&sql_type, dialect).into_strings(),
    }
}

impl Column {
    /// The Python type as written in a row type: `T | None` for a column that can be NULL,
    /// else `python_type`. `Any`, `None` and `T | None` already admit `None`.
    pub fn display_type(&self) -> Option<String> {
        let python_type = self.python_type.as_deref()?;
        let admits_none =
            python_type == ANY || python_type == "None" || python_type.ends_with(" | None");
        Some(if self.nullable() && !admits_none {
            format!("{python_type} | None")
        } else {
            python_type.to_string()
        })
    }
}

/// Infer the Python return type for a SQL query, e.g. `tuple[int, str]`.
pub fn infer_type(
    sql: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Inferred<Option<String>> {
    let columns = analyze_select(sql, dialect, schema)?;
    // A column of unknown type, `*` the schema cannot resolve included, leaves the row type unknown.
    Ok(columns.and_then(|columns| {
        let types: Option<Vec<String>> = columns.iter().map(Column::display_type).collect();
        let types = types?;
        Some(if types.is_empty() {
            "tuple[()]".to_string()
        } else {
            format!("tuple[{}]", types.join(", "))
        })
    }))
}

/// The result columns of a query, or `None` when it returns no rows or doesn't parse.
pub fn analyze_select(
    sql: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Inferred<Option<Vec<Column>>> {
    analyze_with_diagnostics(sql, dialect, schema).0
}

/// [`analyze_select`], and why columns are untyped: unknown or ambiguous names, unknown
/// functions and types, constructs the analyzer does not type. Name resolution (tables,
/// columns, ambiguity) is only judged when `schema` has tables or views.
pub fn analyze_with_diagnostics(
    sql: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> (Inferred<Option<Vec<Column>>>, Vec<NameDiagnostic>) {
    let dialect = SqlDialect::from_name(dialect);
    let Some(stmt) = parse_first(sql, dialect) else {
        return (Ok(None), Vec::new());
    };
    let mut analyzer = Analyzer::new(
        schema.unwrap_or_else(|| crate::catalog::empty(dialect)),
        dialect,
    );
    analyzer.diag = Diagnostics::collecting();
    let columns = analyzer.statement_columns(&stmt);
    (columns, analyzer.diag.items())
}

/// Why `sql` does not parse in `dialect`, as sqlparser words it; `None` when it parses
/// (empty SQL included).
pub fn parse_error(sql: &str, dialect: &str) -> Option<String> {
    let dialect = SqlDialect::from_name(dialect);
    if parse_query(sql, dialect).is_some() {
        return None;
    }
    let input = parser_input(sql, dialect);
    Some(
        match Parser::parse_sql(dialect.parser_dialect().as_ref(), &input) {
            Ok(_) => "unsupported in this dialect".to_string(),
            Err(err) => {
                let text = err.to_string();
                text.strip_prefix("sql parser error: ")
                    .unwrap_or(&text)
                    .to_string()
            }
        },
    )
}

/// Whether `sql` yields rows: a query, DML with RETURNING, or unparseable SQL.
///
/// Empty SQL, DDL, DML without RETURNING and transaction control return no rows.
pub fn returns_rows(sql: &str, dialect: &str) -> bool {
    match parse_query(sql, SqlDialect::from_name(dialect)) {
        None => true,
        Some(stmts) => stmts.first().is_some_and(statement_returns_rows),
    }
}
