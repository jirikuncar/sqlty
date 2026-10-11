//! sqlty._core — the SQL engine behind `sqlty.core`, compiled with PyO3 and maturin.

mod analyzer;
mod checker;
mod fixer;
mod rewriter;

use std::collections::HashMap;
use std::sync::Mutex;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use rewriter::ImportStyle;

type ColumnTuple = (String, Option<String>, Option<String>);

/// The last schema loaded, as `(schema_sql, dialect, schema)`: callers pass the same one per query.
static LAST_SCHEMA: Mutex<Option<(String, String, analyzer::Schema)>> = Mutex::new(None);

fn schema(schema_sql: Option<&str>, dialect: &str) -> Option<analyzer::Schema> {
    let schema_sql = schema_sql?;
    let mut last = LAST_SCHEMA.lock().unwrap_or_else(|e| e.into_inner());
    match last.as_ref() {
        Some((sql, d, schema)) if sql == schema_sql && d == dialect => Some(schema.clone()),
        _ => {
            let schema = analyzer::load_schema(schema_sql, dialect);
            *last = Some((schema_sql.to_string(), dialect.to_string(), schema.clone()));
            Some(schema)
        }
    }
}

fn check_import_style(import_style: Option<&str>) -> PyResult<()> {
    ImportStyle::parse(import_style)
        .map(drop)
        .map_err(PyValueError::new_err)
}

/// Infer the Python return type for a SQL query.
///
/// Args:
///     sql: SQL query string (e.g., "SELECT id::INTEGER, name::TEXT FROM users")
///     dialect: SQL dialect (postgres, mysql, sqlite, ...)
///     schema_sql: Optional CREATE TABLE statements for schema-aware inference
///
/// Returns:
///     Python type string (e.g., "tuple[int, str]") or None if inference fails
///
/// Raises:
///     RuntimeError: SUM(column) names a column missing from a schema table
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn infer_type(sql: &str, dialect: &str, schema_sql: Option<&str>) -> PyResult<Option<String>> {
    analyzer::infer_type(sql, dialect, &schema(schema_sql, dialect))
        .map_err(PyRuntimeError::new_err)
}

/// Result columns of a query as `(name, python_type, sql_type)` tuples.
///
/// Returns None when the query returns no rows or doesn't parse; unknown types are None.
///
/// Raises:
///     RuntimeError: SUM(column) names a column missing from a schema table
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn analyze_select(
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> PyResult<Option<Vec<ColumnTuple>>> {
    let columns = analyzer::analyze_select(sql, dialect, &schema(schema_sql, dialect))
        .map_err(PyRuntimeError::new_err)?;
    Ok(columns.map(|columns| {
        columns
            .into_iter()
            .map(|col| (col.name, col.python_type, col.sql_type))
            .collect()
    }))
}

/// Whether a query yields rows: a query, DML with RETURNING, or unparseable SQL.
///
/// Empty SQL, DDL, DML without RETURNING and transaction control return no rows.
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres"))]
fn returns_rows(sql: &str, dialect: &str) -> bool {
    analyzer::returns_rows(sql, dialect)
}

/// Rewrite Python source: "--sql\n..." -> SQL[T]("--sql\n...").
///
/// Adds necessary imports (SQL, datetime, uuid) at the top.
/// Never modifies string content — only adds the SQL[T]() wrapper.
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, import_style = None, untyped = false))]
fn rewrite_source(
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    import_style: Option<&str>,
    untyped: bool,
) -> PyResult<String> {
    check_import_style(import_style)?;
    Ok(rewriter::rewrite_source(
        source,
        dialect,
        &schema(schema_sql, dialect),
        import_style,
        untyped,
    ))
}

/// Strip Python source: SQL[T]("--sql\n...") -> "--sql\n...".
///
/// Removes unused imports added by rewrite.
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, import_style = None))]
fn strip_source(source: &str, import_style: Option<&str>) -> PyResult<String> {
    check_import_style(import_style)?;
    Ok(rewriter::strip_source(source, import_style))
}

/// Apply what `sqlty check --fix` fixes; rules set to "ignore" are skipped.
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, import_style = None, untyped = false, rules = None))]
fn fix_source(
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    import_style: Option<&str>,
    untyped: bool,
    rules: Option<HashMap<String, String>>,
) -> PyResult<String> {
    check_import_style(import_style)?;
    Ok(fixer::fix_source(
        source,
        dialect,
        &schema(schema_sql, dialect),
        import_style,
        untyped,
        rules,
    ))
}

/// Expand SELECT * into explicit column list.
///
/// Returns:
///     SQL with expanded columns, or None if expansion is not possible
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn expand_star(sql: &str, dialect: &str, schema_sql: Option<&str>) -> Option<String> {
    analyzer::expand_star(sql, dialect, &schema(schema_sql, dialect))
}

/// Check Python source for SQL type issues.
///
/// Returns:
///     List of diagnostic dicts with keys: rule, message, severity, line
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, rules = None, import_style = None, untyped = false))]
fn check_source<'py>(
    py: Python<'py>,
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    rules: Option<HashMap<String, String>>,
    import_style: Option<&str>,
    untyped: bool,
) -> PyResult<Bound<'py, PyList>> {
    check_import_style(import_style)?;
    let diagnostics = checker::check_source(
        source,
        dialect,
        &schema(schema_sql, dialect),
        rules,
        import_style,
        untyped,
    );
    let list = PyList::empty(py);
    for diag in diagnostics {
        let dict = PyDict::new(py);
        dict.set_item("rule", &diag.rule)?;
        dict.set_item("message", &diag.message)?;
        dict.set_item("severity", &diag.severity)?;
        dict.set_item("line", diag.line)?;
        list.append(dict)?;
    }
    Ok(list)
}

/// `(line, annotation, query)` for each `SQL[T](...)` call in Python source.
///
/// `query` is the `--sql` string without its prefix, or None when the call
/// doesn't hold a one-line `--sql` string.
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, import_style = None))]
fn wrapped_queries(
    source: &str,
    import_style: Option<&str>,
) -> PyResult<Vec<(usize, String, Option<String>)>> {
    check_import_style(import_style)?;
    Ok(checker::wrapped_queries(source, import_style))
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(infer_type, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_select, m)?)?;
    m.add_function(wrap_pyfunction!(returns_rows, m)?)?;
    m.add_function(wrap_pyfunction!(rewrite_source, m)?)?;
    m.add_function(wrap_pyfunction!(strip_source, m)?)?;
    m.add_function(wrap_pyfunction!(fix_source, m)?)?;
    m.add_function(wrap_pyfunction!(expand_star, m)?)?;
    m.add_function(wrap_pyfunction!(check_source, m)?)?;
    m.add_function(wrap_pyfunction!(wrapped_queries, m)?)?;
    Ok(())
}
