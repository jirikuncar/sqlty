//! sqlty._core — the SQL engine behind `sqlty.core`, compiled with PyO3 and maturin.

use sqlty_python::checker;
use sqlty_python::fixer;
use sqlty_python::rewriter;
use sqlty_sql::analyzer;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use rewriter::ImportStyle;
use sqlty_python::rules::RuleConfig;

type ColumnTuple = (String, Option<String>, Option<String>);
type NullableColumnTuple = (
    String,
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<String>,
);
type ParamTuple = (
    usize,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<bool>,
    bool,
);

/// A parsed schema and the tables and views it could not parse.
struct LoadedSchema {
    schema: analyzer::Schema,
    skipped: Vec<analyzer::SkippedObject>,
}

type CachedSchema = (String, String, Arc<LoadedSchema>);

/// Most recently used first. Callers pass the same few schemas call after call, and
/// parsing a large one costs about a millisecond, so keep the last few parsed.
static SCHEMAS: Mutex<Vec<CachedSchema>> = Mutex::new(Vec::new());
const SCHEMAS_KEPT: usize = 8;

/// Move the entry for the schema to the front and return it, if cached.
fn cache_hit(
    cache: &mut [CachedSchema],
    schema_sql: &str,
    dialect: &str,
) -> Option<Arc<LoadedSchema>> {
    let at = cache
        .iter()
        .position(|(sql, d, _)| sql == schema_sql && d == dialect)?;
    cache[..=at].rotate_right(1);
    Some(Arc::clone(&cache[0].2))
}

fn lock_schemas() -> MutexGuard<'static, Vec<CachedSchema>> {
    SCHEMAS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The parsed schema. The cache is not locked while parsing, so a slow schema does not hold
/// up callers of other schemas; two threads may parse the same one, and the first to finish
/// is kept.
fn cached_schema(schema_sql: &str, dialect: &str) -> Arc<LoadedSchema> {
    if let Some(hit) = cache_hit(&mut lock_schemas(), schema_sql, dialect) {
        return hit;
    }
    let (schema, skipped) = analyzer::load_schema_with_skipped(schema_sql, dialect);
    let loaded = Arc::new(LoadedSchema { schema, skipped });
    let mut cache = lock_schemas();
    if let Some(hit) = cache_hit(&mut cache, schema_sql, dialect) {
        return hit;
    }
    cache.insert(
        0,
        (
            schema_sql.to_string(),
            dialect.to_string(),
            Arc::clone(&loaded),
        ),
    );
    cache.truncate(SCHEMAS_KEPT);
    loaded
}

fn schema(schema_sql: Option<&str>, dialect: &str) -> Option<analyzer::Schema> {
    Some(cached_schema(schema_sql?, dialect).schema.clone())
}

/// The schema, unless `source` cannot hold a query: every query, wrapper and
/// diagnostic involves the text `sql` (`--sql`, `SQL[...]`), in any case.
fn schema_for(source: &str, schema_sql: Option<&str>, dialect: &str) -> Option<analyzer::Schema> {
    if source
        .as_bytes()
        .windows(3)
        .any(|w| w.eq_ignore_ascii_case(b"sql"))
    {
        schema(schema_sql, dialect)
    } else {
        None
    }
}

fn check_import_style(import_style: Option<&str>) -> PyResult<()> {
    ImportStyle::parse(import_style)
        .map(drop)
        .map_err(PyValueError::new_err)
}

/// Parse `rules` (rule name to `error`, `warn` or `ignore`) once, at the boundary.
fn rule_config(rules: Option<HashMap<String, String>>) -> PyResult<RuleConfig> {
    RuleConfig::from_overrides(rules).map_err(|e| PyValueError::new_err(e.to_string()))
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
    analyzer::infer_type(sql, dialect, schema(schema_sql, dialect).as_deref())
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))
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
    let columns = analyzer::analyze_select(sql, dialect, schema(schema_sql, dialect).as_deref())
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    Ok(columns.map(|columns| {
        columns
            .into_iter()
            .map(|col| (col.name.clone(), col.display_type(), col.sql_type))
            .collect()
    }))
}

/// Result columns of a query as `(name, python_type, sql_type, nullable, json_shape)` tuples.
///
/// `python_type` is never spelled `T | None` here, unlike in `analyze_select`: `nullable` says
/// whether the column can be NULL (True), cannot (False), or is not known (None: an
/// unresolved column, an unknown function), so row types can be judged against it.
/// Returns None when the query returns no rows or doesn't parse.
///
/// Raises:
///     RuntimeError: SUM(column) names a column missing from a schema table
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn analyze_columns(
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> PyResult<Option<Vec<NullableColumnTuple>>> {
    let columns = analyzer::analyze_select(sql, dialect, schema(schema_sql, dialect).as_deref())
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    Ok(columns.map(|columns| {
        columns
            .into_iter()
            .map(|col| {
                let nullable = match col.nullability {
                    analyzer::Nullability::Nullable => Some(true),
                    analyzer::Nullability::NonNull => Some(false),
                    analyzer::Nullability::Unknown => None,
                };
                (
                    col.name,
                    col.python_type,
                    col.sql_type,
                    nullable,
                    col.json.map(|shape| shape.to_json()),
                )
            })
            .collect()
    }))
}

/// Parameters of a query as `(index, name, python_type, sql_type, nullable, cast)` tuples.
///
/// A placeholder (`%s`, `%(name)s`, `$1`, `?`, `:name`) takes its type from its context: the
/// column it is compared with or stored in, a cast, `LIMIT`, `= ANY(%s)`. `index` is
/// 1-based; `name` is set for named placeholders. `nullable` is False when the parameter is
/// stored in a `NOT NULL` column, True for a nullable one, None when not known. `cast` is
/// True when the type is that of an explicit cast, which accepts any value. Types are
/// None when the context does not say. Returns None when the query doesn't parse.
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn analyze_params(sql: &str, dialect: &str, schema_sql: Option<&str>) -> Option<Vec<ParamTuple>> {
    let params = analyzer::analyze_params(sql, dialect, schema(schema_sql, dialect).as_deref())?;
    Some(
        params
            .into_iter()
            .map(|param| {
                let nullable = match param.nullability {
                    analyzer::Nullability::Nullable => Some(true),
                    analyzer::Nullability::NonNull => Some(false),
                    analyzer::Nullability::Unknown => None,
                };
                (
                    param.index,
                    param.name,
                    param.python_type,
                    param.sql_type,
                    nullable,
                    param.cast,
                )
            })
            .collect(),
    )
}

/// The rules `query_diagnostics` reports, which are off by default.
#[pyfunction]
fn name_rules() -> Vec<&'static str> {
    sqlty_sql::diagnostics::NameRule::ALL
        .iter()
        .map(|rule| rule.name())
        .collect()
}

/// Why a query has untyped columns, as `(rule, message)` pairs: `unknown-table`,
/// `unknown-column`, `ambiguous-column`, `unknown-function`, `unknown-type` and
/// `unsupported-syntax`. Tables, columns and ambiguity are only judged with a schema.
///
/// Example:
///     query_diagnostics("SELECT nope FROM users", schema_sql="CREATE TABLE users (id INT)")
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn query_diagnostics(sql: &str, dialect: &str, schema_sql: Option<&str>) -> Vec<(String, String)> {
    analyzer::analyze_with_diagnostics(sql, dialect, schema(schema_sql, dialect).as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name().to_string(), found.message))
        .collect()
}

/// Names of the schema tables that could not be parsed, even without their
/// constraints; their columns are unknown to inference.
#[pyfunction]
#[pyo3(signature = (schema_sql, *, dialect = "postgres"))]
fn schema_skipped_tables(schema_sql: &str, dialect: &str) -> Vec<String> {
    skipped_objects(schema_sql, dialect, Some("table"))
        .into_iter()
        .map(|(_, name)| name)
        .collect()
}

/// `(kind, name)` of the schema tables and views that could not be parsed, `kind` being
/// `"table"` or `"view"`; their columns are unknown to inference.
#[pyfunction]
#[pyo3(signature = (schema_sql, *, dialect = "postgres"))]
fn schema_skipped_objects(schema_sql: &str, dialect: &str) -> Vec<(String, String)> {
    skipped_objects(schema_sql, dialect, None)
}

fn skipped_objects(schema_sql: &str, dialect: &str, only: Option<&str>) -> Vec<(String, String)> {
    cached_schema(schema_sql, dialect)
        .skipped
        .iter()
        .filter(|(kind, _)| only.is_none_or(|only| only == *kind))
        .map(|(kind, name)| ((*kind).to_string(), name.clone()))
        .collect()
}

/// Why a query does not parse, as sqlparser words it; None when it parses (or is empty).
///
/// Example:
///     parse_error("SELECT id FROM", dialect="postgres")  # "Expected: ..., found: EOF ..."
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres"))]
fn parse_error(sql: &str, dialect: &str) -> Option<String> {
    analyzer::parse_error(sql, dialect)
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
/// With keep_lines, the imports are joined with "; " onto existing lines so
/// line numbers stay the same (used by the `sqlty` codec).
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, import_style = None, untyped = false, keep_lines = false))]
fn rewrite_source(
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    import_style: Option<&str>,
    untyped: bool,
    keep_lines: bool,
) -> PyResult<String> {
    check_import_style(import_style)?;
    Ok(rewriter::rewrite_source_with(
        source,
        dialect,
        schema_for(source, schema_sql, dialect).as_deref(),
        import_style,
        untyped,
        keep_lines,
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
/// `suppressed` maps a line to the rules a `# sqlty: ignore` comment on it suppresses
/// (`None` for all); fixes of those rules on that line are not applied.
///
/// Raises:
///     ValueError: unsupported import_style, or `rules` names an unknown rule or severity
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, import_style = None, untyped = false, rules = None, suppressed = None))]
fn fix_source(
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    import_style: Option<&str>,
    untyped: bool,
    rules: Option<HashMap<String, String>>,
    suppressed: Option<rewriter::Suppressed>,
) -> PyResult<String> {
    check_import_style(import_style)?;
    let rules = rule_config(rules)?;
    Ok(fixer::fix_source(
        source,
        dialect,
        schema_for(source, schema_sql, dialect).as_deref(),
        import_style,
        untyped,
        &rules,
        suppressed.unwrap_or_default(),
    ))
}

/// Expand SELECT * into explicit column list.
///
/// Returns:
///     SQL with expanded columns, or None if expansion is not possible
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn expand_star(sql: &str, dialect: &str, schema_sql: Option<&str>) -> Option<String> {
    analyzer::expand_star(sql, dialect, schema(schema_sql, dialect).as_deref())
}

/// Check Python source for SQL type issues.
///
/// `any_calls` are the byte offsets of the `SQL[T](...)` calls whose `T` is
/// `typing.Any`; they are not reported as `unchecked-query`.
///
/// Returns:
///     List of diagnostic dicts with keys: rule, message, severity, line
///
/// Raises:
///     ValueError: unsupported import_style, or `rules` names an unknown rule or severity
#[pyfunction]
#[pyo3(signature = (source, *, dialect = "postgres", schema_sql = None, rules = None, import_style = None, untyped = false, any_calls = vec![]))]
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts a Vec, not a slice"
)]
fn check_source<'py>(
    py: Python<'py>,
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    rules: Option<HashMap<String, String>>,
    import_style: Option<&str>,
    untyped: bool,
    any_calls: Vec<usize>,
) -> PyResult<Bound<'py, PyList>> {
    check_import_style(import_style)?;
    let rules = rule_config(rules)?;
    let diagnostics = checker::check_source(
        source,
        dialect,
        schema_for(source, schema_sql, dialect).as_deref(),
        &rules,
        import_style,
        untyped,
        &any_calls,
    );
    let list = PyList::empty(py);
    for diag in diagnostics {
        let dict = PyDict::new(py);
        dict.set_item("rule", diag.rule.name())?;
        dict.set_item("message", &diag.message)?;
        dict.set_item("severity", diag.severity.as_str())?;
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

/// Each query `check` sees, for `sqlty check --statistics`: `(line, kind, text)`.
///
/// `kind` is `"query"` with the `--sql` text (marker included), or the rule that
/// skips the string (`fstring-skipped`, `tstring-skipped`, `unchecked-query`), with
/// empty text. Queries passed to an `Annotated[LiteralString, SQL]` helper are not
/// included, except those carrying the marker. `any_calls` is as for
/// `check_source`.
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, import_style = None, any_calls = vec![]))]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts a Vec, not a slice"
)]
fn query_sites(
    source: &str,
    import_style: Option<&str>,
    any_calls: Vec<usize>,
) -> PyResult<Vec<(usize, String, String)>> {
    check_import_style(import_style)?;
    Ok(checker::query_sites(source, import_style, &any_calls)
        .into_iter()
        .map(|(line, kind, text)| (line, kind.name().to_string(), text))
        .collect())
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(infer_type, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_select, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_columns, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_params, m)?)?;
    m.add_function(wrap_pyfunction!(query_diagnostics, m)?)?;
    m.add_function(wrap_pyfunction!(name_rules, m)?)?;
    m.add_function(wrap_pyfunction!(schema_skipped_tables, m)?)?;
    m.add_function(wrap_pyfunction!(schema_skipped_objects, m)?)?;
    m.add_function(wrap_pyfunction!(parse_error, m)?)?;
    m.add_function(wrap_pyfunction!(returns_rows, m)?)?;
    m.add_function(wrap_pyfunction!(rewrite_source, m)?)?;
    m.add_function(wrap_pyfunction!(strip_source, m)?)?;
    m.add_function(wrap_pyfunction!(fix_source, m)?)?;
    m.add_function(wrap_pyfunction!(expand_star, m)?)?;
    m.add_function(wrap_pyfunction!(check_source, m)?)?;
    m.add_function(wrap_pyfunction!(wrapped_queries, m)?)?;
    m.add_function(wrap_pyfunction!(query_sites, m)?)?;
    Ok(())
}
