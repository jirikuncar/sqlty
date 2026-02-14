//! sqlty._core — Rust implementation of the core computation functions.
//!
//! This module is compiled as a native Python extension via PyO3 + maturin.
//! It replaces `sqlty._core_py` (pure Python fallback) with identical behavior
//! but significantly better performance.
//!
//! The Python import-switching facade in `sqlty/core.py`:
//! ```python
//! try:
//!     from sqlty._core import infer_type, rewrite_source, ...  # Rust (fast)
//! except ImportError:
//!     from sqlty._core_py import infer_type, rewrite_source, ...  # Python fallback
//! ```

mod analyzer;
mod checker;
mod rewriter;

use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// Infer the Python return type for a SQL query.
///
/// Args:
///     sql: SQL query string (e.g., "SELECT id::INTEGER, name::TEXT FROM users")
///     dialect: SQL dialect (postgres, mysql, sqlite)
///     schema_sql: Optional CREATE TABLE statements for schema-aware inference
///
/// Returns:
///     Python type string (e.g., "tuple[int, str]") or None if inference fails
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn infer_type(sql: &str, dialect: &str, schema_sql: Option<&str>) -> Option<String> {
    let schema = schema_sql.map(|s| analyzer::load_schema(s, dialect));
    analyzer::infer_type(sql, dialect, &schema)
}

/// Rewrite Python source: "--sql\n..." -> SQL[T]("--sql\n...").
///
/// Adds necessary imports (SQL, datetime, uuid) at the top.
/// Never modifies string content — only adds the SQL[T]() wrapper.
///
/// Args:
///     source: Python source code
///     dialect: SQL dialect
///     schema_sql: Optional CREATE TABLE statements
///
/// Returns:
///     Rewritten source code
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None))]
fn rewrite_source(source: &str, dialect: &str, schema_sql: Option<&str>) -> String {
    let schema = schema_sql.map(|s| analyzer::load_schema(s, dialect));
    rewriter::rewrite_source(source, dialect, &schema)
}

/// Strip Python source: SQL[T]("--sql\n...") -> "--sql\n...".
///
/// Removes unused imports added by rewrite.
///
/// Args:
///     source: Python source code with SQL[T](...) wrappers
///
/// Returns:
///     Stripped source code
#[pyfunction]
#[pyo3(signature = (source,))]
fn strip_source(source: &str) -> String {
    rewriter::strip_source(source)
}

/// Expand SELECT * into explicit column list.
///
/// Args:
///     sql: SQL query containing SELECT *
///     dialect: SQL dialect
///     schema_sql: CREATE TABLE statements (required for expansion)
///
/// Returns:
///     SQL with expanded columns, or None if expansion is not possible
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn expand_star(sql: &str, dialect: &str, schema_sql: Option<&str>) -> Option<String> {
    let schema = schema_sql.map(|s| analyzer::load_schema(s, dialect));
    analyzer::expand_star(sql, dialect, &schema)
}

/// Check Python source for SQL type issues.
///
/// Args:
///     source: Python source code
///     dialect: SQL dialect
///     schema_sql: Optional CREATE TABLE statements
///     rules: Rule severity overrides (e.g., {"select-star": "ignore"})
///
/// Returns:
///     List of diagnostic dicts with keys: rule, message, severity, line
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, rules = None))]
fn check_source<'py>(
    py: Python<'py>,
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    rules: Option<HashMap<String, String>>,
) -> Bound<'py, PyList> {
    let schema = schema_sql.map(|s| analyzer::load_schema(s, dialect));
    let diagnostics = checker::check_source(source, dialect, &schema, rules);

    let list = PyList::empty(py);
    for diag in diagnostics {
        let dict = PyDict::new(py);
        dict.set_item("rule", &diag.rule).unwrap();
        dict.set_item("message", &diag.message).unwrap();
        dict.set_item("severity", &diag.severity).unwrap();
        dict.set_item("line", diag.line).unwrap();
        list.append(dict).unwrap();
    }

    list
}

/// PyO3 module definition — this becomes `sqlty._core`.
#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(infer_type, m)?)?;
    m.add_function(wrap_pyfunction!(rewrite_source, m)?)?;
    m.add_function(wrap_pyfunction!(strip_source, m)?)?;
    m.add_function(wrap_pyfunction!(expand_star, m)?)?;
    m.add_function(wrap_pyfunction!(check_source, m)?)?;
    Ok(())
}
