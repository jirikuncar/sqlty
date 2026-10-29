//! The analyzer's public entry points: type inference, column analysis, parse checks.

use std::sync::Arc;

use sqlparser::ast::Statement;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Location, Span};

use crate::diagnostics::{Diagnostics, NameDiagnostic};

use super::dialect::SqlDialect;
use super::dml::statement_returns_rows;
use super::expr::from_sql_type;
use super::input::{parser_input, Input};
use super::names::alias_name;
use super::preprocess::{parse_input_as_written, parse_query, too_deep, MAX_DEPTH};
use super::query::{Analyzer, Column};
use super::star_expand::statement_stars;
use super::{Inferred, Schema, SchemaData, SkippedObject, ANY};

/// Load schema from CREATE TABLE, VIEW, DOMAIN and TYPE ... AS ENUM statements; in
/// PostgreSQL also ALTER, DROP, search_path and functions (see `crate::catalog`).
/// Returns the `(kind, name)` of the tables that could not be parsed even without
/// their constraints and of the views that could not be parsed.
pub fn load_schema_with_skipped(schema_sql: &str, dialect: &str) -> (Schema, Vec<SkippedObject>) {
    let (catalog, skipped) = crate::catalog::load(schema_sql, dialect);
    (Arc::new(catalog), skipped)
}

/// [`load_schema_with_skipped`], with the Python types the project sets for SQL types and
/// columns: a column of an overridden type, or an overridden column, and a cast to an
/// overridden type, have that Python type.
pub fn load_schema_with_overrides(
    schema_sql: &str,
    dialect: &str,
    overrides: crate::catalog::TypeOverrides,
) -> (Schema, Vec<SkippedObject>) {
    let (catalog, skipped) = crate::catalog::load_with(schema_sql, dialect, Arc::new(overrides));
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

/// Fields of the composite type `name` the schema defines (`CREATE TYPE point AS (x int,
/// y int)`), in declaration order, typed as columns; `None` when it is not one.
pub fn composite_columns(name: &str, dialect: &str, schema: &SchemaData) -> Option<Vec<Column>> {
    let dialect = SqlDialect::from_name(dialect);
    let fields = schema.composite(&name.to_lowercase())?;
    Some(
        fields
            .iter()
            .map(|field| {
                Column::from_typed(
                    field.name.clone(),
                    field.typed(dialect),
                    super::nullability::Nullability::declared(field.not_null),
                )
            })
            .collect(),
    )
}

/// Infer the Python return type for a SQL query, e.g. `tuple[int, str]`.
pub fn infer_type(
    sql: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Inferred<Option<String>> {
    Ok(analyze_select(sql, dialect, schema)?.and_then(|columns| row_type(&columns)))
}

/// The row type of result columns, as [`infer_type`] spells it; `None` when a column's type,
/// or a `*` the schema cannot resolve, is unknown.
pub fn row_type(columns: &[Column]) -> Option<String> {
    let types: Option<Vec<String>> = columns.iter().map(Column::display_type).collect();
    let types = types?;
    Some(if types.is_empty() {
        "tuple[()]".to_string()
    } else {
        format!("tuple[{}]", types.join(", "))
    })
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
    parse(sql, dialect).analyze_with_diagnostics(schema)
}

/// Why `sql` does not parse in `dialect`, as sqlparser words it; `None` when it parses
/// (empty SQL included).
pub fn parse_error(sql: &str, dialect: &str) -> Option<String> {
    parse(sql, dialect).error()
}

/// Parse a query once to ask several things of it, where [`parse_error`],
/// [`analyze_with_diagnostics`] and [`result_stars`](super::result_stars) would each
/// parse it again.
///
/// ```
/// use sqlty_sql::analyzer::parse;
///
/// let parsed = parse("SELECT * FROM t", "postgres");
/// assert_eq!(parsed.error(), None);
/// assert_eq!(parsed.result_stars(), Some(vec!["SELECT *".to_string()]));
/// assert!(parse("SELEC 1", "postgres").error().is_some());
/// ```
pub fn parse(sql: &str, dialect: &str) -> Parsed {
    let dialect = SqlDialect::from_name(dialect);
    let input = parser_input(sql, dialect).into_owned();
    let (stmts, as_written) = parse_input_as_written(input.text(), dialect);
    Parsed {
        input,
        stmts,
        dialect,
        as_written,
    }
}

/// A query as [`parse`] read it.
pub struct Parsed {
    /// The query sqlparser read, from [`parser_input`].
    input: Input<'static>,
    stmts: Option<Vec<Statement>>,
    dialect: SqlDialect,
    /// Whether sqlparser read the input as written, so its spans point into it.
    as_written: bool,
}

impl Parsed {
    /// [`parse_error`] of the query.
    pub fn error(&self) -> Option<String> {
        if self.stmts.is_some() {
            return None;
        }
        Some(
            match Parser::parse_sql(self.dialect.parser_dialect().as_ref(), &self.input.source) {
                Ok(stmts) if too_deep(&stmts) => {
                    format!("expressions nest more than {MAX_DEPTH} levels deep")
                }
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

    /// [`analyze_with_diagnostics`] of the query.
    pub fn analyze_with_diagnostics(
        &self,
        schema: Option<&SchemaData>,
    ) -> (Inferred<Option<Vec<Column>>>, Vec<NameDiagnostic>) {
        let Some(stmt) = self.first() else {
            return (Ok(None), Vec::new());
        };
        let mut analyzer = Analyzer::new(
            schema.unwrap_or_else(|| crate::catalog::empty(self.dialect)),
            self.dialect,
        );
        analyzer.diag = Diagnostics::collecting();
        analyzer.source = text_source(&self.input.source, self.dialect);
        let columns = analyzer.statement_columns(stmt);
        let found = analyzer.diag.located().into_iter().map(|(mut found, at)| {
            found.span = self.as_written.then(|| self.bytes(at)).flatten();
            found
        });
        (columns, found.collect())
    }

    /// The bytes of the query sqlparser read at `span`.
    fn bytes(&self, span: Span) -> Option<std::ops::Range<usize>> {
        let text = self.input.text();
        let offset = |at: Location| {
            let line = usize::try_from(at.line).ok()?.checked_sub(1)?;
            let column = usize::try_from(at.column).ok()?.checked_sub(1)?;
            let start = match line {
                0 => 0,
                _ => text.match_indices('\n').nth(line - 1)?.0 + 1,
            };
            let rest = &text[start..];
            let within = rest
                .char_indices()
                .map(|(i, _)| i)
                .chain([rest.len()])
                .nth(column)?;
            Some(start + within)
        };
        let (start, end) = (offset(span.start)?, offset(span.end)?);
        (start < end).then(|| self.input.original(start, false)..self.input.original(end, true))
    }

    /// [`result_stars`](super::result_stars) of the query.
    pub fn result_stars(&self) -> Option<Vec<String>> {
        let mut stars = Vec::new();
        statement_stars(self.first()?, &mut stars);
        Some(stars)
    }

    fn first(&self) -> Option<&Statement> {
        self.stmts.as_ref()?.first()
    }
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

/// The SQL as parsed, `input` from [`parser_input`], in the dialects that name a column by
/// its expression's text.
pub(super) fn text_source(input: &str, dialect: SqlDialect) -> Option<std::rc::Rc<str>> {
    matches!(dialect, SqlDialect::MySql | SqlDialect::Sqlite).then(|| std::rc::Rc::from(input))
}
