//! The sqlty engine for the browser, as the docs playground runs it.
//!
//! [`run`] reads a JSON request and answers in JSON. Built for `wasm32-unknown-unknown`, the
//! module exports `buffer(len)` (where to write a request of `len` bytes), `run()` (answers
//! it, returning the answer's length) and `output()` (where the answer is). Offsets are UTF-16 code units, as
//! JavaScript strings count them.
//!
//! The target aborts on a panic: `run()` traps, leaving the instance unusable, and `output()`
//! and `output_len()` give the panic's message. The host asks a new instance again, with the
//! message as `crashed`.
//!
//! # Example
//!
//! ```
//! let answer = sqlty_wasm::run(
//!     r#"{"op": "analyze", "source": "q = \"--sql\\nSELECT 1 AS n\"", "dialect": "postgres"}"#,
//! );
//! let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
//! assert_eq!(answer["queries"][0]["row_type"], "tuple[int]");
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlty_python::breaking::breaking;
use sqlty_python::checker::{query_sites, Diagnostic, SiteKind};
use sqlty_python::config;
use sqlty_python::fixer::fix_module;
use sqlty_python::locate::{self, Queries};
use sqlty_python::migrations::check_schema;
use sqlty_python::options::Options;
use sqlty_python::params::add_param_types;
use sqlty_python::project::{self, FindError, Read};
use sqlty_python::rewriter::{rewrite_source_with, strip_source};
use sqlty_python::rules::RuleConfig;
use sqlty_python::rules::{Rule, Severity};
use sqlty_python::types::symbols::Source;
use sqlty_python::{check, rows, suppress};
use sqlty_sql::analyzer::{
    explain_select, load_schema_with_overrides, load_schema_with_skipped, parse, route, row_type,
    CandidateKind, Nullability, RuleRef, Schema, SchemaData, TypeOverrides,
};
use sqlty_sql::highlight::Highlight;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Op {
    Config,
    Project,
    #[serde(untagged)]
    Source(SourceOp),
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum SourceOp {
    Analyze,
    Complete,
    Hover,
    Format,
    Breaking,
    Query,
}

#[derive(Deserialize)]
struct Request {
    op: Op,
    #[serde(default)]
    source: String,
    /// The bare SQL of `query`.
    #[serde(default)]
    sql: String,
    #[serde(default)]
    schema: String,
    /// A name for `schema` that a later request may send in place of its text: one without
    /// the text gets `{"need": "schema"}` once the engine no longer holds it.
    #[serde(default)]
    schema_key: Option<String>,
    /// The schema before a change to `schema`, for `breaking`.
    #[serde(default)]
    before: String,
    #[serde(default = "postgres")]
    dialect: String,
    #[serde(default)]
    offset: usize,
    /// A `sqlty.toml`: its `dialect` wins over the request's.
    #[serde(default)]
    config: String,
    /// A sqlty table the host already resolved (`extend` applied), as `config` answers it.
    #[serde(default)]
    settings: Option<serde_json::Map<String, Value>>,
    /// The path of the file `config` reads `source` from, for its messages; its name picks the
    /// table (`[tool.sqlty]` of a `pyproject.toml`).
    #[serde(default)]
    path: String,
    /// Whether `config` reports a `pyproject.toml` or `Cargo.toml` without a sqlty table.
    #[serde(default)]
    required: bool,
    /// The files `project` reads, by absolute path: the text, or `null` for no such file.
    #[serde(default)]
    files: HashMap<String, Option<String>>,
    /// The directory `settings` came from, as `project` answers its `file`: with `file`, the
    /// settings are those of `file`, its database and overrides applied.
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    file: Option<String>,
    /// What the instance that ran this request before panicked with: `analyze` reports it as
    /// `internal-error`, another op as an `error`.
    #[serde(default)]
    crashed: Option<String>,
    /// A message to panic with in place of answering, for a host to test its recovery: a request
    /// that `crashed` does not.
    #[serde(default)]
    panic: Option<String>,
}

fn postgres() -> String {
    "postgres".to_string()
}

/// Answer a JSON request about the Python `source`, read against `schema` in `dialect`
/// (`postgres` by default).
///
/// - `analyze`: `diagnostics` (what `sqlty check` reports for a module by itself, each
///   with the `fix` edits of its quick fix), `schema_diagnostics` (what it reports on
///   `schema`, such as `tenant-policy`, offsets into `schema`), `queries` (row type, columns
///   and parameters of each query), `highlights` (the SQL tokens to color), and what
///   `sqlty rewrite`, `sqlty strip` and `sqlty check --fix` make of the source: `rewritten`,
///   `stripped` and `fixed`.
/// - `complete`: the completions at `offset`, and the `range` they replace; `null` outside
///   a query.
/// - `hover`: what the word at `offset` is, in Markdown; `null` when nothing.
/// - `format`: `formatted`, what `sqlty format` makes of the source. Not part of `analyze`,
///   which runs on every keystroke.
/// - `query`: `columns`, `params` and `diagnostics` of the bare SQL `sql`, for a host in any
///   language; offsets into `sql`. With `schema_key`, `schema` is sent once, not per query.
/// - `breaking`: `diagnostics` the source gets against `schema` but not against `before`,
///   as `sqlty breaking` reports what a schema change breaks.
/// - `config`: the sqlty table of `source`, the text of the file at `path`, validated as
///   `sqlty check` validates it, as `{"config": table}`; `null` for a `pyproject.toml` or
///   `Cargo.toml` without one, unless `required`.
/// - `project`: the configuration of the directory `path`, as `sqlty check` finds it from
///   `files`: `{"config": table, "file": path}` with `extend` applied, its `schema` and
///   `queries` relative to the directory of `file`; `null` when no file has one;
///   `{"need": path}` for a file `files` lacks, which the host reads and asks again with;
///   `{"warning": message}` for a `pyproject.toml` or `Cargo.toml` that is not TOML, which
///   `sqlty check` skips. The host passes the table as `settings`.
///
/// A request that `crashed` gets the `internal-error` of `sqlty check` in place of a check.
///
/// `config` is a `sqlty.toml` whose `dialect`, `driver`, `import-style`, `param-types`,
/// `preview`, `max-query-variants`, `rules`, `types`, `deprecated-columns` and `tenancy` apply; another key, or an invalid one, is an `error`, as `sqlty check` reports
/// it. `settings` replaces it with a table `config` answered, `extend` applied.
pub fn run(request: &str) -> String {
    let error = |error: String| json!({ "error": error }).to_string();
    let request: Request = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(found) => return error(found.to_string()),
    };
    let op = match request.op {
        Op::Source(op) => op,
        Op::Config => {
            let path = request.path.as_str();
            let name = path.rsplit('/').next().unwrap_or(path);
            return match config::load(request.source.as_bytes(), name, path, request.required) {
                Ok(found) => json!(found.map(|found| json!({ "config": found.table }))).to_string(),
                Err(found) => error(found.to_string()),
            };
        }
        Op::Project => return project(&request.path, &request.files).to_string(),
    };
    let settings = match &request.settings {
        Some(table) => Ok(resolved(&routed(
            table,
            request.base.as_ref(),
            request.file.as_ref(),
        ))),
        None => settings(&request.config),
    };
    let settings = match settings {
        Ok(settings) => settings,
        Err(found) => return error(found),
    };
    let rules = match RuleConfig::from_overrides(Some(settings.rules)) {
        Ok(rules) => rules,
        Err(found) => return error(found.to_string()),
    };
    let source = request.source.as_str();
    let units = sqlty_python::utf16::Utf16::new(source);
    let utf16 = |byte: usize| units.at(byte);
    if let Some(cause) = &request.crashed {
        let found = check::crashed(cause, &rules).diagnostics;
        let found: Vec<_> = found.into_iter().map(|found| found.diagnostic).collect();
        return match op {
            SourceOp::Analyze => json!({
                "diagnostics": reported(&found, utf16),
                "queries": [],
                "highlights": [],
                "rewritten": source,
                "stripped": source,
                "fixed": source,
            })
            .to_string(),
            _ => error(format!(
                "sqlty crashed: {cause}; please report it at {}",
                sqlty_python::report::issue_url(cause)
            )),
        };
    }
    if let Some(message) = request.panic {
        #[expect(clippy::panic, reason = "the request asks for it")]
        std::panic::panic_any(message);
    }
    let dialect = settings.dialect.as_deref().unwrap_or(&request.dialect);
    let param_types = settings.param_types;
    let overrides = match TypeOverrides::new(&settings.types, dialect) {
        Ok(overrides) => overrides,
        Err(found) => return error(format!("[tool.sqlty.types]: {found}")),
    };
    let load = |text: &str, key: Option<&str>| {
        cached(key.unwrap_or(text), dialect, &settings.types, || {
            (!text.trim().is_empty()).then(|| match overrides.is_empty() {
                true => load_schema_with_skipped(text, dialect).0,
                false => load_schema_with_overrides(text, dialect, overrides.clone()).0,
            })
        })
    };
    let schema = load(&request.schema, request.schema_key.as_deref());
    if request.schema_key.is_some() && schema.is_none() {
        return json!({ "need": "schema" }).to_string();
    }
    let before = matches!(op, SourceOp::Breaking)
        .then(|| load(&request.before, None))
        .flatten();
    let mut options = match Options::new(
        dialect,
        schema.as_deref(),
        settings.import_style.as_deref(),
        false,
    ) {
        Ok(options) => options,
        Err(found) => return error(found.to_string()),
    };
    let check = |options: &Options<'_>, schema: &Option<Schema>| {
        let has_schema = schema
            .as_ref()
            .is_some_and(|schema| !schema.databases().is_some_and(|all| all.absent));
        checked(
            source,
            dialect,
            options,
            &rules,
            has_schema,
            settings.variants,
        )
    };
    let offset = byte_offset(source, request.offset);
    let queries = Queries::new(source, &options.style, &[]);
    let params = |source: String| match param_types {
        true => add_param_types(&source, &options, false),
        false => source,
    };
    let answer = match op {
        SourceOp::Analyze => {
            let diagnostics = check(&options, &schema);
            let schema_units = sqlty_python::utf16::Utf16::new(&request.schema);
            let schema_diagnostics: Vec<Diagnostic> = schema
                .as_deref()
                .map(|loaded| check_schema(loaded, &[(request.schema.as_str(), &rules)]))
                .into_iter()
                .flatten()
                .flatten()
                .collect();
            json!({
            "diagnostics": reported(&diagnostics, utf16),
            "schema_diagnostics": reported(&schema_diagnostics, |byte| schema_units.at(byte)),
            "queries": query_sites(source, &options.style, &[])
                .into_iter()
                .filter(|(_, kind, _)| *kind == SiteKind::Query)
                .map(|(line, _, sql)| described(line, &sql, dialect, &options))
                .collect::<Vec<_>>(),
            "highlights": queries
                .highlights(dialect)
                .into_iter()
                .map(|(range, kind)| json!([utf16(range.start), utf16(range.end), highlight(kind)]))
                .collect::<Vec<_>>(),
            "rewritten": params(rewrite_source_with(source, &options, false)),
            "stripped": strip_source(source, &options.style),
            "fixed": params(fix_module(source, Source::new(source, VERSION).as_ref(), &options, &rules, None, None)),
            })
        }
        SourceOp::Complete => match locate::complete(&queries, offset, dialect, options.schema, 50)
        {
            Some(found) => json!({
                "start": utf16(found.replace.start),
                "end": utf16(found.replace.end),
                "items": found.candidates.iter().map(|candidate| json!({
                    "label": candidate.label,
                    "kind": candidate_kind(candidate.kind),
                    "detail": candidate.detail,
                    "sql_type": candidate.sql_type,
                    "nullable": candidate.nullable,
                })).collect::<Vec<_>>(),
            }),
            None => Value::Null,
        },
        SourceOp::Hover => match locate::hover(&queries, offset, dialect, options.schema) {
            Some(found) => json!({
                "start": utf16(found.range.start),
                "end": utf16(found.range.end),
                "contents": found.contents,
            }),
            None => Value::Null,
        },
        SourceOp::Format => {
            json!({ "formatted": sqlty_format::format_source(source, dialect, &options.style) })
        }
        SourceOp::Query => query(&request.sql, dialect, options.schema, &rules),
        SourceOp::Breaking => {
            let against = check(&options, &schema);
            options.schema = before.as_deref();
            let own = check(&options, &before);
            let key = |found: &Diagnostic| (found.line, found.rule.name(), found.message.clone());
            let found = breaking(&own, against, key, |found| found.rule.name());
            json!({ "diagnostics": reported(&found, utf16) })
        }
    };
    answer.to_string()
}

struct Settings {
    dialect: Option<String>,
    import_style: Option<String>,
    param_types: bool,
    /// `max-query-variants`, with `preview`.
    variants: Option<usize>,
    rules: HashMap<String, String>,
    types: Vec<(String, String)>,
}

/// The settings of a table `config` answered; a `driver` adds its `types` under the table's.
fn resolved(table: &serde_json::Map<String, Value>) -> Settings {
    let text = |key: &str| table.get(key).and_then(Value::as_str).map(String::from);
    let pairs = |key: &str| {
        table
            .get(key)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
            .collect::<Vec<_>>()
    };
    let dialect = text("dialect");
    let mut types = match text("driver") {
        Some(driver) => config::driver_types(&driver, dialect.as_deref().unwrap_or("postgres")),
        None => Vec::new(),
    };
    types.extend(pairs("types"));
    let configured = ["deprecated-columns", "tenancy"]
        .into_iter()
        .filter_map(|key| {
            let value = serde_json::from_value::<toml::Value>(table.get(key)?.clone()).ok()?;
            Some((key.to_string(), value))
        });
    types.extend(config::deprecated_types(&toml::Table::from_iter(
        configured,
    )));
    Settings {
        dialect,
        import_style: text("import-style"),
        param_types: table.get("param-types") != Some(&Value::Bool(false)),
        variants: (table.get("preview") == Some(&Value::Bool(true))).then(|| {
            let most = table.get("max-query-variants").and_then(Value::as_u64);
            most.map_or(MAX_QUERY_VARIANTS, |most| {
                usize::try_from(most).unwrap_or(usize::MAX)
            })
        }),
        rules: config::rule_entries(
            table
                .get("rules")
                .and_then(|rules| serde_json::from_value::<toml::Value>(rules.clone()).ok())
                .as_ref(),
        )
        .into_iter()
        .collect(),
        types,
    }
}

/// The keys the engine reads: the others name files, which the page has none of.
const READ: [&str; 10] = [
    "deprecated-columns",
    "dialect",
    "driver",
    "import-style",
    "max-query-variants",
    "param-types",
    "preview",
    "rules",
    "tenancy",
    "types",
];

/// `sqlty_project::settings::MAX_QUERY_VARIANTS`.
const MAX_QUERY_VARIANTS: usize = 64;

/// The settings of the `sqlty.toml` text `config`, validated as `sqlty check` validates it.
fn settings(config: &str) -> Result<Settings, String> {
    let table = config::load(config.as_bytes(), "sqlty.toml", "sqlty.toml", true)
        .map_err(|error| error.to_string())?
        .map(|config| config.table)
        .unwrap_or_default();
    if let Some(key) = table.keys().find(|key| !READ.contains(&key.as_str())) {
        return Err(format!(
            "sqlty.toml: the WebAssembly engine reads only {}, not '{key}'",
            READ.join(", ")
        ));
    }
    let table = table
        .into_iter()
        .map(|(key, value)| (key, json!(value)))
        .collect();
    Ok(resolved(&table))
}

/// The Python the page is read as.
const VERSION: (u8, u8) = (3, 14);

/// What `sqlty check` reports for the module `source`: its queries, the row types it
/// names for them, the calls of its helpers and of its drivers, less what a
/// `# sqlty: ignore` comment suppresses. A source that does not parse gets what its
/// strings alone tell.
fn checked(
    source: &str,
    dialect: &str,
    options: &Options<'_>,
    rules: &RuleConfig,
    has_schema: bool,
    variants: Option<usize>,
) -> Vec<Diagnostic> {
    let coerce = HashSet::new();
    let settings = rows::Settings {
        options,
        rules,
        index: None,
        module: None,
        coerce: &coerce,
    };
    let lint = |text: &str| {
        sqlty_format::lint_source(text, dialect, &options.style)
            .into_iter()
            .map(|found| check::Lint {
                message: format!("{}: {}", found.code, found.message),
                span: found.span,
                query_start: found.query_start,
            })
            .collect()
    };
    let run = check::Run {
        settings: &settings,
        has_schema,
        lint: &lint,
        helpers: None,
        variants,
    };
    let module = Source::new(source, VERSION).map(Arc::new);
    let found = check::module(source, module.as_ref(), &run);
    let ignores = match &module {
        Some(module) => suppress::ignores(module),
        None => suppress::unparsed_ignores(source),
    };
    let needed = found.needed;
    suppress::apply(&ignores, found.diagnostics, rules)
        .into_iter()
        // A field one query needs `| None` for is not redundant for another.
        .filter(|reported| {
            !reported
                .redundant
                .as_ref()
                .is_some_and(|key| needed.contains(key))
        })
        .map(|reported| reported.diagnostic)
        .collect()
}

/// `diagnostics` as `analyze` answers them, `utf16` the offset of a byte.
fn reported(diagnostics: &[Diagnostic], utf16: impl Fn(usize) -> usize) -> Vec<Value> {
    diagnostics
        .iter()
        .map(|found| {
            json!({
                "rule": found.rule.name(),
                "message": found.message,
                "severity": found.severity.as_str(),
                "line": found.line,
                "start": utf16(found.span.start),
                "end": utf16(found.span.end),
                "fix": found.fix.iter().map(|edit| json!({
                    "start": utf16(edit.range.start),
                    "end": utf16(edit.range.end),
                    "replacement": edit.replacement,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// The row type, columns and parameters of the query `sql` on `line`.
fn described(line: usize, sql: &str, dialect: &str, options: &Options<'_>) -> Value {
    let (routed, schema) = route(sql, dialect, options.schema);
    let parsed = parse(sql, routed);
    let columns = parsed.columns(schema);
    let params = parsed.params(sql, schema).unwrap_or_default();
    let explained = explain_select(sql, dialect, options.schema).unwrap_or_default();
    let rule = |rule: Option<RuleRef>| {
        rule.map(
            |rule| json!({"id": rule.id, "reference": rule.reference, "reasoning": rule.reasoning}),
        )
    };
    json!({
        "line": line,
        "sql": sql,
        "row_type": columns.as_deref().and_then(row_type),
        "columns": columns.iter().flatten().enumerate().map(|(i, column)| json!({
            "name": column.name,
            "type": column.display_type(),
            "sql_type": column.sql_type,
            "type_rule": rule(explained.get(i).and_then(|found| found.type_rule)),
            "nullability_rule": rule(explained.get(i).and_then(|found| found.nullability_rule)),
        })).collect::<Vec<_>>(),
        "params": params.iter().map(|param| json!({
            "index": param.index,
            "name": param.name,
            "type": param.python_type,
            "sql_type": param.sql_type,
            "nullable": match param.nullability {
                Nullability::Nullable => Some(true),
                Nullability::NonNull => Some(false),
                Nullability::Unknown => None,
            },
        })).collect::<Vec<_>>(),
    })
}

type SchemaKey = (String, String, Vec<(String, String)>);

thread_local! {
    /// The schemas loaded last, so a host asking about query after query with one schema,
    /// as a linter does, loads it once.
    static SCHEMAS: std::cell::RefCell<Vec<(SchemaKey, Schema)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The schema `key` (its text, or the host's name for it) names in `dialect` with the `types`
/// overrides: one of the last two loaded, else what `load` makes of it.
fn cached(
    key: &str,
    dialect: &str,
    types: &[(String, String)],
    load: impl FnOnce() -> Option<Schema>,
) -> Option<Schema> {
    SCHEMAS.with_borrow_mut(|schemas| {
        let hit = schemas
            .iter()
            .position(|((k, d, o), _)| k == key && d == dialect && o == types);
        let entry = match hit {
            Some(at) => schemas.remove(at),
            None => (
                (key.to_string(), dialect.to_string(), types.to_vec()),
                load()?,
            ),
        };
        let schema = entry.1.clone();
        schemas.insert(0, entry);
        schemas.truncate(2);
        Some(schema)
    })
}

/// The answer of `query`: what one parse of `sql` tells, as `check` reports it.
fn query(sql: &str, dialect: &str, schema: Option<&SchemaData>, rules: &RuleConfig) -> Value {
    let units = sqlty_python::utf16::Utf16::new(sql);
    let (routed, schema) = route(sql, dialect, schema);
    let parsed = parse(sql, routed);
    let nullable = |nullability: Nullability| match nullability {
        Nullability::Nullable => Some(true),
        Nullability::NonNull => Some(false),
        Nullability::Unknown => None,
    };
    let diagnostic = |rule: Rule, message: String, span: Range<usize>, fix: Option<String>| {
        let severity = rules.severity(rule);
        (severity != Severity::Ignore).then(|| {
            let (start, end) = (units.at(span.start), units.at(span.end));
            json!({
                "rule": rule.name(),
                "message": message,
                "severity": severity.as_str(),
                "start": start,
                "end": end,
                "fix": fix.map(|replacement| json!({"start": start, "end": end, "replacement": replacement})).into_iter().collect::<Vec<_>>(),
            })
        })
    };
    if let Some(error) = parsed.error() {
        let found = diagnostic(
            Rule::UnparsedQuery,
            format!("query does not parse: {error}"),
            0..sql.len(),
            None,
        );
        return json!({"returns_rows": true, "columns": null, "params": [], "diagnostics": found.into_iter().collect::<Vec<_>>()});
    }
    let (columns, found) = parsed.analyze_with_diagnostics(schema);
    let params = parsed.params(sql, schema).unwrap_or_default();
    let diagnostics: Vec<Value> = found
        .into_iter()
        .filter(|found| !found.shadowed_by.is_some_and(|rule| rules.is_enabled(rule)))
        .filter_map(|found| {
            let span = found.span.unwrap_or(0..sql.len());
            diagnostic(found.rule, found.message, span, found.replacement)
        })
        .collect();
    json!({
        "returns_rows": parsed.returns_rows(),
        "columns": columns.map(|columns| columns.iter().map(|column| json!({
            "name": column.name,
            "sql_type": column.sql_type.as_deref().or_else(|| constant_sql_type(column.python_type.as_deref()?)),
            "nullable": nullable(column.nullability),
        })).collect::<Vec<_>>()),
        "params": params.iter().map(|param| json!({
            "index": param.index,
            "name": param.name,
            "sql_type": param.sql_type,
            "nullable": nullable(param.nullability),
        })).collect::<Vec<_>>(),
        "diagnostics": diagnostics,
    })
}

/// The SQL type of a constant, which the analyzer types in Python only: `1`, `'a'`, `1.5`.
fn constant_sql_type(python_type: &str) -> Option<&'static str> {
    let base = match python_type.strip_prefix("typing.Literal[") {
        Some(values) if values.starts_with(['\'', '"']) => "str",
        Some(_) => "int",
        None => python_type,
    };
    const TYPES: [(&str, &str); 5] = [
        ("int", "INT"),
        ("str", "TEXT"),
        ("bool", "BOOLEAN"),
        ("float", "DOUBLE"),
        ("decimal.Decimal", "DECIMAL"),
    ];
    TYPES
        .iter()
        .find(|(python, _)| *python == base)
        .map(|(_, sql)| *sql)
}

/// The byte of `source` at `units` UTF-16 code units, clamped to its end.
fn byte_offset(source: &str, units: usize) -> usize {
    let mut counted = 0;
    for (byte, char) in source.char_indices() {
        if counted >= units {
            return byte;
        }
        counted += char.len_utf16();
    }
    source.len()
}

fn highlight(kind: Highlight) -> &'static str {
    match kind {
        Highlight::Keyword => "keyword",
        Highlight::Function => "function",
        Highlight::String => "string",
        Highlight::Number => "number",
        Highlight::Comment => "comment",
        Highlight::Operator => "operator",
        Highlight::Parameter => "parameter",
    }
}

fn candidate_kind(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Table => "table",
        CandidateKind::View => "view",
        CandidateKind::Cte => "cte",
        CandidateKind::Column => "column",
        CandidateKind::Join => "join",
    }
}

#[cfg(target_arch = "wasm32")]
mod abi {
    use std::cell::RefCell;
    use std::sync::Once;

    thread_local! {
        static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static OUTPUT: RefCell<String> = const { RefCell::new(String::new()) };
    }

    #[no_mangle]
    extern "C" fn buffer(len: usize) -> *mut u8 {
        INPUT.with_borrow_mut(|input| {
            input.clear();
            input.resize(len, 0);
            input.as_mut_ptr()
        })
    }

    static HOOK: Once = Once::new();

    #[no_mangle]
    extern "C" fn run() -> usize {
        HOOK.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                let cause = info.payload_as_str().unwrap_or("unknown cause");
                OUTPUT.with(|output| {
                    if let Ok(mut output) = output.try_borrow_mut() {
                        *output = cause.to_string();
                    }
                });
            }));
        });
        OUTPUT.with_borrow_mut(String::clear);
        let answer = INPUT.with_borrow(|input| match std::str::from_utf8(input) {
            Ok(request) => super::run(request),
            Err(error) => serde_json::json!({ "error": error.to_string() }).to_string(),
        });
        OUTPUT.with_borrow_mut(|output| {
            *output = answer;
            output.len()
        })
    }

    #[no_mangle]
    extern "C" fn output() -> *const u8 {
        OUTPUT.with_borrow(|output| output.as_ptr())
    }

    #[no_mangle]
    extern "C" fn output_len() -> usize {
        OUTPUT.with_borrow(String::len)
    }
}

/// `table` as `file` reads it, `base` the directory of its configuration.
fn routed(
    table: &serde_json::Map<String, Value>,
    base: Option<&String>,
    file: Option<&String>,
) -> serde_json::Map<String, Value> {
    let (Some(base), Some(file)) = (base, file) else {
        return table.clone();
    };
    let Ok(project) = serde_json::from_value::<toml::Table>(Value::Object(table.clone())) else {
        return table.clone();
    };
    let missing = |_: &Path| Read::Missing;
    let found = project::route(&project, Path::new(base), Path::new(file), &missing);
    json!(project::file_table(&project, &found))
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// The `project` answer for `directory`, reading `files`.
fn project(directory: &str, files: &HashMap<String, Option<String>>) -> Value {
    let read = |path: &Path| match path.to_str().and_then(|path| files.get(path)) {
        Some(Some(text)) => Read::Text(text.as_bytes().to_vec()),
        Some(None) => Read::Missing,
        None => Read::Unread,
    };
    match project::find(Path::new(directory), &read) {
        Ok(Some(found)) => json!({ "config": found.config.table, "file": found.file }),
        Ok(None) => Value::Null,
        Err(FindError::Need(path)) => json!({ "need": path }),
        Err(FindError::Syntax(message)) => json!({ "warning": message }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests;
