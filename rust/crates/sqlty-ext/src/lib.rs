//! `sqlty._core`: the SQL engine behind `sqlty.core`, compiled with PyO3 and maturin.
//!
//! This crate is only the Python binding. The analysis lives in `sqlty-sql` and the source
//! handling in `sqlty-python`; each `#[pyfunction]` here converts its arguments, releases the
//! GIL (`Python::detach`) while the engine runs, and converts the result to tuples and lists
//! that `sqlty/core.py` wraps into dataclasses.
//!
//! - Inference: `infer_type`, `analyze_select`, `analyze_columns`, `analyze_params`,
//!   `expand_star`, `query_diagnostics`, `parse_error`, `returns_rows`.
//! - Source: `rewrite_source`, `strip_source`, `fix_source`, `check_source`,
//!   `wrapped_queries`, `query_sites`.
//! - Schema: `schema_skipped_tables`, `schema_skipped_objects`, `name_rules`.
//!
//! Parsed schemas are cached per `(schema text, dialect)` in a small LRU shared by all
//! threads, because callers pass the same few schemas call after call. The stub
//! `sqlty/_core.pyi` must match the signatures here.
//!
//! The crate is a `cdylib`, so it has no Rust API and no doc tests; the unit tests cover
//! the pieces that need no interpreter (the schema cache, `schema_for`, nullability
//! mapping). The `#[pyfunction]`s are exercised from Python by `tests/`.

use sqlty_python::checker;
use sqlty_python::fixer;
use sqlty_python::rewriter;
use sqlty_sql::analyzer;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use rewriter::ImportStyle;
use sqlty_python::options::{ConfigError, Options};
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
    SCHEMAS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// What an entry point can fail with, and the Python exception each becomes.
#[derive(Debug, thiserror::Error)]
enum Error {
    /// `ValueError`: an unsupported `import_style`, or an unknown rule or severity.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// `RuntimeError`: inference found a query it cannot judge.
    #[error(transparent)]
    Infer(#[from] analyzer::InferError),
}

impl From<Error> for PyErr {
    fn from(error: Error) -> Self {
        match error {
            Error::Config(_) => PyValueError::new_err(error.to_string()),
            Error::Infer(_) => PyRuntimeError::new_err(error.to_string()),
        }
    }
}

/// Whether a column or parameter can be NULL (True), cannot (False), or is not known (None).
fn nullable(nullability: analyzer::Nullability) -> Option<bool> {
    match nullability {
        analyzer::Nullability::Nullable => Some(true),
        analyzer::Nullability::NonNull => Some(false),
        analyzer::Nullability::Unknown => None,
    }
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
fn infer_type(
    py: Python<'_>,
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> Result<Option<String>, Error> {
    Ok(py.detach(|| analyzer::infer_type(sql, dialect, schema(schema_sql, dialect).as_deref()))?)
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
    py: Python<'_>,
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> Result<Option<Vec<ColumnTuple>>, Error> {
    let columns = py.detach(|| {
        analyzer::analyze_select(sql, dialect, schema(schema_sql, dialect).as_deref())
    })?;
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
    py: Python<'_>,
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> Result<Option<Vec<NullableColumnTuple>>, Error> {
    let columns = py.detach(|| {
        analyzer::analyze_select(sql, dialect, schema(schema_sql, dialect).as_deref())
    })?;
    Ok(columns.map(|columns| {
        columns
            .into_iter()
            .map(|col| {
                (
                    col.name,
                    col.python_type,
                    col.sql_type,
                    nullable(col.nullability),
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
fn analyze_params(
    py: Python<'_>,
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> Option<Vec<ParamTuple>> {
    let params = py.detach(|| {
        analyzer::analyze_params(sql, dialect, schema(schema_sql, dialect).as_deref())
    })?;
    Some(
        params
            .into_iter()
            .map(|param| {
                (
                    param.index,
                    param.name,
                    param.python_type,
                    param.sql_type,
                    nullable(param.nullability),
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
fn query_diagnostics(
    py: Python<'_>,
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> Vec<(String, String)> {
    py.detach(|| {
        analyzer::analyze_with_diagnostics(sql, dialect, schema(schema_sql, dialect).as_deref())
            .1
            .into_iter()
            .map(|found| (found.rule.name().to_string(), found.message))
            .collect()
    })
}

/// Names of the schema tables that could not be parsed, even without their
/// constraints; their columns are unknown to inference.
#[pyfunction]
#[pyo3(signature = (schema_sql, *, dialect = "postgres"))]
fn schema_skipped_tables(py: Python<'_>, schema_sql: &str, dialect: &str) -> Vec<String> {
    py.detach(|| skipped_objects(schema_sql, dialect, Some("table")))
        .into_iter()
        .map(|(_, name)| name)
        .collect()
}

/// `(kind, name)` of the schema tables and views that could not be parsed, `kind` being
/// `"table"` or `"view"`; their columns are unknown to inference.
#[pyfunction]
#[pyo3(signature = (schema_sql, *, dialect = "postgres"))]
fn schema_skipped_objects(
    py: Python<'_>,
    schema_sql: &str,
    dialect: &str,
) -> Vec<(String, String)> {
    py.detach(|| skipped_objects(schema_sql, dialect, None))
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
fn parse_error(py: Python<'_>, sql: &str, dialect: &str) -> Option<String> {
    py.detach(|| analyzer::parse_error(sql, dialect))
}

/// Whether a query yields rows: a query, DML with RETURNING, or unparseable SQL.
///
/// Empty SQL, DDL, DML without RETURNING and transaction control return no rows.
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres"))]
fn returns_rows(py: Python<'_>, sql: &str, dialect: &str) -> bool {
    py.detach(|| analyzer::returns_rows(sql, dialect))
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
    py: Python<'_>,
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    import_style: Option<&str>,
    untyped: bool,
    keep_lines: bool,
) -> Result<String, Error> {
    py.detach(|| {
        let schema = schema_for(source, schema_sql, dialect);
        let options = Options::new(dialect, schema.as_deref(), import_style, untyped)?;
        Ok(rewriter::rewrite_source_with(source, &options, keep_lines))
    })
}

/// Strip Python source: SQL[T]("--sql\n...") -> "--sql\n...".
///
/// Removes unused imports added by rewrite.
///
/// Raises:
///     ValueError: unsupported import_style
#[pyfunction]
#[pyo3(signature = (source, *, import_style = None))]
fn strip_source(py: Python<'_>, source: &str, import_style: Option<&str>) -> Result<String, Error> {
    py.detach(|| {
        let style = ImportStyle::parse(import_style)?;
        Ok(rewriter::strip_source(source, &style))
    })
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
#[expect(
    clippy::too_many_arguments,
    reason = "the keyword-only Python signature is the API"
)]
fn fix_source(
    py: Python<'_>,
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    import_style: Option<&str>,
    untyped: bool,
    rules: Option<HashMap<String, String>>,
    suppressed: Option<rewriter::Suppressed>,
) -> Result<String, Error> {
    py.detach(|| {
        let schema = schema_for(source, schema_sql, dialect);
        let options = Options::new(dialect, schema.as_deref(), import_style, untyped)?;
        let rules = RuleConfig::from_overrides(rules).map_err(ConfigError::from)?;
        Ok(fixer::fix_source(
            source,
            &options,
            &rules,
            suppressed.unwrap_or_default(),
        ))
    })
}

/// Expand SELECT * into explicit column list.
///
/// Returns:
///     SQL with expanded columns, or None if expansion is not possible
#[pyfunction]
#[pyo3(signature = (sql, *, dialect = "postgres", schema_sql = None))]
fn expand_star(
    py: Python<'_>,
    sql: &str,
    dialect: &str,
    schema_sql: Option<&str>,
) -> Option<String> {
    py.detach(|| analyzer::expand_star(sql, dialect, schema(schema_sql, dialect).as_deref()))
}

/// A finding of `check_source`, which Python reads as a dict.
#[derive(IntoPyObject)]
struct DiagnosticOut {
    rule: &'static str,
    message: String,
    severity: &'static str,
    line: usize,
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
#[expect(
    clippy::too_many_arguments,
    reason = "the keyword-only Python signature is the API"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts a Vec, not a slice"
)]
fn check_source(
    py: Python<'_>,
    source: &str,
    dialect: &str,
    schema_sql: Option<&str>,
    rules: Option<HashMap<String, String>>,
    import_style: Option<&str>,
    untyped: bool,
    any_calls: Vec<usize>,
) -> Result<Vec<DiagnosticOut>, Error> {
    py.detach(|| {
        let schema = schema_for(source, schema_sql, dialect);
        let options = Options::new(dialect, schema.as_deref(), import_style, untyped)?;
        let rules = RuleConfig::from_overrides(rules).map_err(ConfigError::from)?;
        Ok(checker::check_source(source, &options, &rules, &any_calls)
            .into_iter()
            .map(|diag| DiagnosticOut {
                rule: diag.rule.name(),
                message: diag.message,
                severity: diag.severity.as_str(),
                line: diag.line,
            })
            .collect())
    })
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
    py: Python<'_>,
    source: &str,
    import_style: Option<&str>,
) -> Result<Vec<(usize, String, Option<String>)>, Error> {
    py.detach(|| {
        let style = ImportStyle::parse(import_style)?;
        Ok(checker::wrapped_queries(source, &style))
    })
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
    py: Python<'_>,
    source: &str,
    import_style: Option<&str>,
    any_calls: Vec<usize>,
) -> Result<Vec<(usize, String, String)>, Error> {
    py.detach(|| {
        let style = ImportStyle::parse(import_style)?;
        Ok(checker::query_sites(source, &style, &any_calls)
            .into_iter()
            .map(|(line, kind, text)| (line, kind.name().to_string(), text))
            .collect())
    })
}

/// No pyclasses, and the only global state is the schema cache behind a `Mutex`, so the
/// module runs without the GIL on free-threaded builds.
#[pymodule(gil_used = false)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(sql: &str, dialect: &str) -> CachedSchema {
        let (schema, skipped) = analyzer::load_schema_with_skipped(sql, dialect);
        (
            sql.to_string(),
            dialect.to_string(),
            Arc::new(LoadedSchema { schema, skipped }),
        )
    }

    #[test]
    fn nullable_maps_the_three_states() {
        assert_eq!(nullable(analyzer::Nullability::Nullable), Some(true));
        assert_eq!(nullable(analyzer::Nullability::NonNull), Some(false));
        assert_eq!(nullable(analyzer::Nullability::Unknown), None);
    }

    #[test]
    fn cache_hit_moves_the_entry_to_the_front() {
        let mut cache = vec![
            entry("CREATE TABLE a (x INT)", "postgres"),
            entry("CREATE TABLE b (x INT)", "postgres"),
            entry("CREATE TABLE c (x INT)", "postgres"),
        ];
        let hit = cache_hit(&mut cache, "CREATE TABLE c (x INT)", "postgres");
        assert!(hit.is_some());
        let order: Vec<&str> = cache.iter().map(|(sql, _, _)| sql.as_str()).collect();
        assert_eq!(
            order,
            [
                "CREATE TABLE c (x INT)",
                "CREATE TABLE a (x INT)",
                "CREATE TABLE b (x INT)"
            ]
        );
    }

    #[test]
    fn cache_hit_needs_the_same_sql_and_dialect() {
        let mut cache = vec![entry("CREATE TABLE a (x INT)", "postgres")];
        assert!(cache_hit(&mut cache, "CREATE TABLE a (x INT)", "mysql").is_none());
        assert!(cache_hit(&mut cache, "CREATE TABLE a (y INT)", "postgres").is_none());
        assert!(cache_hit(&mut [], "CREATE TABLE a (x INT)", "postgres").is_none());
    }

    #[test]
    fn cache_hit_shares_the_parsed_schema() {
        let mut cache = vec![entry("CREATE TABLE a (x INT)", "postgres")];
        let first = cache_hit(&mut cache, "CREATE TABLE a (x INT)", "postgres").unwrap();
        let second = cache_hit(&mut cache, "CREATE TABLE a (x INT)", "postgres").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    /// One test, because the cache is a process-wide static: a test filling it would evict
    /// the entries another test expects to find.
    #[test]
    fn cached_schema_parses_once_and_keeps_a_bounded_number() {
        let sql = "CREATE TABLE sqlty_ext_cache_once (x INT)";
        let first = cached_schema(sql, "postgres");
        assert!(Arc::ptr_eq(&first, &cached_schema(sql, "postgres")));
        assert!(!Arc::ptr_eq(&first, &cached_schema(sql, "mysql")));

        for n in 0..SCHEMAS_KEPT * 2 {
            cached_schema(
                &format!("CREATE TABLE sqlty_ext_bound_{n} (x INT)"),
                "postgres",
            );
        }
        assert!(lock_schemas().len() <= SCHEMAS_KEPT);
    }

    #[test]
    fn schema_is_absent_without_schema_sql() {
        assert!(schema(None, "postgres").is_none());
        assert!(schema(Some("CREATE TABLE sqlty_ext_present (x INT)"), "postgres").is_some());
    }

    #[test]
    fn schema_for_skips_sources_that_cannot_hold_a_query() {
        let ddl = Some("CREATE TABLE sqlty_ext_for (x INT)");
        assert!(schema_for("print('hello')", ddl, "postgres").is_none());
        assert!(schema_for("q = SQL[int]('SELECT 1')", ddl, "postgres").is_some());
        assert!(schema_for("x = '--SQL\\nSELECT 1'", ddl, "postgres").is_some());
        assert!(schema_for("SQL", None, "postgres").is_none());
        assert!(schema_for("", ddl, "postgres").is_none());
    }

    #[test]
    fn skipped_objects_lists_what_the_schema_could_not_parse() {
        let sql = "CREATE TABLE sqlty_ext_ok (x INT); CREATE TABLE sqlty_ext_bad (x INT,;";
        let all = skipped_objects(sql, "postgres", None);
        assert!(
            all.iter().all(|(_, name)| name != "sqlty_ext_ok"),
            "a parsed table is not skipped: {all:?}"
        );
        let tables = skipped_objects(sql, "postgres", Some("table"));
        let views = skipped_objects(sql, "postgres", Some("view"));
        assert!(views.is_empty());
        assert_eq!(tables, all);
    }

    #[test]
    fn errors_render_the_message_of_their_source() {
        let error = Error::from(ConfigError::ImportStyle("nope".to_string()));
        assert_eq!(error.to_string(), "Unsupported import-style: 'nope'");
    }
}
