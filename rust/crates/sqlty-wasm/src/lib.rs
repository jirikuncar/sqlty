//! The sqlty engine for the browser, as the docs playground runs it.
//!
//! [`run`] reads a JSON request and answers in JSON. Built for `wasm32-unknown-unknown`, the
//! module exports `buffer(len)` (where to write a request of `len` bytes), `run()` (answers
//! it, returning the answer's length) and `output()` (where the answer is). Offsets are UTF-16 code units, as
//! JavaScript strings count them.
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlty_python::checker::{query_sites, Diagnostic, SiteKind};
use sqlty_python::config;
use sqlty_python::fixer::fix_source;
use sqlty_python::locate::{self, Queries};
use sqlty_python::options::Options;
use sqlty_python::params::add_param_types;
use sqlty_python::rewriter::{rewrite_source_with, strip_source, Suppressed};
use sqlty_python::rules::RuleConfig;
use sqlty_python::types::symbols::Source;
use sqlty_python::{check, rows, suppress};
use sqlty_sql::analyzer::{
    analyze_params, analyze_select, explain_select, load_schema_with_overrides,
    load_schema_with_skipped, row_type, CandidateKind, Nullability, RuleRef, TypeOverrides,
};
use sqlty_sql::highlight::Highlight;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Op {
    Analyze,
    Complete,
    Hover,
    Format,
    Config,
}

#[derive(Deserialize)]
struct Request {
    op: Op,
    source: String,
    #[serde(default)]
    schema: String,
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
}

/// Answer a JSON request about the Python `source`, read against `schema` in `dialect`.
///
/// - `analyze`: `diagnostics` (what `sqlty check` reports for a module by itself, each
///   with the `fix` edits of its quick fix), `queries` (row type, columns
///   and parameters of each query), `highlights` (the SQL tokens to color), and what
///   `sqlty rewrite`, `sqlty strip` and `sqlty check --fix` make of the source: `rewritten`,
///   `stripped` and `fixed`.
/// - `complete`: the completions at `offset`, and the `range` they replace; `null` outside
///   a query.
/// - `hover`: what the word at `offset` is, in Markdown; `null` when nothing.
/// - `format`: `formatted`, what `sqlty format` makes of the source. Not part of `analyze`,
///   which runs on every keystroke.
/// - `config`: the sqlty table of `source`, the text of the file at `path`, validated as
///   `sqlty check` validates it, as `{"config": table}`; `null` for a `pyproject.toml` or
///   `Cargo.toml` without one, unless `required`. A host finds and merges the files
///   (`extend`), then passes the result as `settings`.
///
/// `config` is a `sqlty.toml` whose `dialect`, `driver`, `import-style`, `param-types`, `rules`
/// and `types` apply; another key, or an invalid one, is an `error`, as `sqlty check` reports
/// it. `settings` replaces it with a table `config` answered, `extend` applied.
pub fn run(request: &str) -> String {
    let error = |error: String| json!({ "error": error }).to_string();
    let request: Request = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(found) => return error(found.to_string()),
    };
    if let Op::Config = request.op {
        let path = request.path.as_str();
        let name = path.rsplit('/').next().unwrap_or(path);
        return match config::load(request.source.as_bytes(), name, path, request.required) {
            Ok(found) => json!(found.map(|found| json!({ "config": found.table }))).to_string(),
            Err(found) => error(found.to_string()),
        };
    }
    let settings = match &request.settings {
        Some(table) => Ok(resolved(table)),
        None => settings(&request.config),
    };
    let settings = match settings {
        Ok(settings) => settings,
        Err(found) => return error(found),
    };
    let dialect = settings.dialect.as_deref().unwrap_or(&request.dialect);
    let param_types = settings.param_types;
    let overrides = match TypeOverrides::new(&settings.types, dialect) {
        Ok(overrides) => overrides,
        Err(found) => return error(format!("[tool.sqlty.types]: {found}")),
    };
    let schema = (!request.schema.trim().is_empty()).then(|| match overrides.is_empty() {
        true => load_schema_with_skipped(&request.schema, dialect).0,
        false => load_schema_with_overrides(&request.schema, dialect, overrides).0,
    });
    let options = Options::new(
        dialect,
        schema.as_deref(),
        settings.import_style.as_deref(),
        false,
    );
    let (options, rules) = match options
        .and_then(|options| Ok((options, RuleConfig::from_overrides(Some(settings.rules))?)))
    {
        Ok(found) => found,
        Err(found) => return error(found.to_string()),
    };
    let source = request.source.as_str();
    let utf16 = |byte: usize| source[..byte].encode_utf16().count();
    let offset = byte_offset(source, request.offset);
    let queries = Queries::new(source, &options.style, &[]);
    let params = |source: String| match param_types {
        true => add_param_types(&source, &options),
        false => source,
    };
    let answer = match request.op {
        Op::Analyze => {
            let has_schema = schema
                .as_ref()
                .is_some_and(|schema| !schema.databases().is_some_and(|all| all.absent));
            let diagnostics = checked(source, dialect, &options, &rules, has_schema);
            json!({
            "diagnostics": diagnostics
                .iter()
                .map(|found| json!({
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
                }))
                .collect::<Vec<_>>(),
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
            "fixed": params(fix_source(source, &options, &rules, &Suppressed::new())),
            })
        }
        Op::Complete => match locate::complete(&queries, offset, dialect, options.schema, 50) {
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
        Op::Hover => match locate::hover(&queries, offset, dialect, options.schema) {
            Some(found) => json!({
                "start": utf16(found.range.start),
                "end": utf16(found.range.end),
                "contents": found.contents,
            }),
            None => Value::Null,
        },
        Op::Format => {
            json!({ "formatted": sqlty_format::format_source(source, dialect, &options.style) })
        }
        Op::Config => unreachable!("answered above"),
    };
    answer.to_string()
}

struct Settings {
    dialect: Option<String>,
    import_style: Option<String>,
    param_types: bool,
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
    Settings {
        dialect,
        import_style: text("import-style"),
        param_types: table.get("param-types") == Some(&Value::Bool(true)),
        rules: pairs("rules").into_iter().collect(),
        types,
    }
}

/// The keys the engine reads: the others name files, which the page has none of.
const READ: [&str; 6] = [
    "dialect",
    "driver",
    "import-style",
    "param-types",
    "rules",
    "types",
];

/// The settings of the `sqlty.toml` text `config`, validated as `sqlty check` validates it.
fn settings(config: &str) -> Result<Settings, String> {
    let table = config::load(config.as_bytes(), "sqlty.toml", "sqlty.toml", true)
        .map_err(|error| error.to_string())?
        .map(|config| config.table)
        .unwrap_or_default();
    if let Some(key) = table.keys().find(|key| !READ.contains(&key.as_str())) {
        return Err(format!(
            "sqlty.toml: the playground reads only {}, not '{key}'",
            READ.join(", ")
        ));
    }
    match json!(table) {
        Value::Object(table) => Ok(resolved(&table)),
        _ => unreachable!("a TOML table is a JSON object"),
    }
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
        variants: None,
    };
    let module = Source::new(source, VERSION).map(Arc::new);
    let found = match &module {
        Some(module) => check::diagnostics(module, &run),
        None => check::unparsed(source, &run),
    };
    let ignores = module.as_deref().map(suppress::ignores).unwrap_or_default();
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

/// The row type, columns and parameters of the query `sql` on `line`.
fn described(line: usize, sql: &str, dialect: &str, options: &Options<'_>) -> Value {
    let columns = analyze_select(sql, dialect, options.schema).ok().flatten();
    let params = analyze_params(sql, dialect, options.schema).unwrap_or_default();
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

    #[no_mangle]
    extern "C" fn run() -> usize {
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
}

#[cfg(test)]
mod tests;
