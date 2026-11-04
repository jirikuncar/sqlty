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

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};
use sqlty_python::checker::{check_source, query_sites, SiteKind};
use sqlty_python::config;
use sqlty_python::locate::{self, Queries};
use sqlty_python::options::Options;
use sqlty_python::rewriter::rewrite_source_with;
use sqlty_python::rules::RuleConfig;
use sqlty_sql::analyzer::{
    analyze_params, analyze_select, load_schema_with_skipped, row_type, CandidateKind, Nullability,
};
use sqlty_sql::highlight::Highlight;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Op {
    Analyze,
    Complete,
    Hover,
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
}

/// Answer a JSON request about the Python `source`, read against `schema` in `dialect`.
///
/// - `analyze`: `diagnostics` (what `sqlty check` reports, without the row types it reads
///   from Python), `queries` (row type, columns and parameters of each query),
///   `highlights` (the SQL tokens to color) and `rewritten` (what `sqlty rewrite` makes).
/// - `complete`: the completions at `offset`, and the `range` they replace; `null` outside
///   a query.
/// - `hover`: what the word at `offset` is, in Markdown; `null` when nothing.
///
/// `config` is a `sqlty.toml` whose `dialect`, `import-style` and `rules` apply; another key,
/// or an invalid one, is an `error`, as `sqlty check` reports it.
pub fn run(request: &str) -> String {
    let error = |error: String| json!({ "error": error }).to_string();
    let request: Request = match serde_json::from_str(request) {
        Ok(request) => request,
        Err(found) => return error(found.to_string()),
    };
    let settings = match settings(&request.config) {
        Ok(settings) => settings,
        Err(found) => return error(found),
    };
    let dialect = settings.dialect.as_deref().unwrap_or(&request.dialect);
    let schema = (!request.schema.trim().is_empty())
        .then(|| load_schema_with_skipped(&request.schema, dialect).0);
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
    let answer = match request.op {
        Op::Analyze => json!({
            "diagnostics": check_source(source, &options, &rules, &[])
                .iter()
                .map(|found| json!({
                    "rule": found.rule.name(),
                    "message": found.message,
                    "severity": found.severity.as_str(),
                    "start": utf16(found.span.start),
                    "end": utf16(found.span.end),
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
            "rewritten": rewrite_source_with(source, &options, false),
        }),
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
    };
    answer.to_string()
}

struct Settings {
    dialect: Option<String>,
    import_style: Option<String>,
    rules: HashMap<String, String>,
}

/// The settings of the `sqlty.toml` text `config`, validated as `sqlty check` validates it.
fn settings(config: &str) -> Result<Settings, String> {
    let table = config::load(config.as_bytes(), "sqlty.toml", "sqlty.toml", true)
        .map_err(|error| error.to_string())?
        .map(|config| config.table)
        .unwrap_or_default();
    // The other keys name files, which the page has none of.
    if let Some(key) = table
        .keys()
        .find(|key| !["dialect", "import-style", "rules"].contains(&key.as_str()))
    {
        return Err(format!(
            "sqlty.toml: the playground reads only dialect, import-style, rules, not '{key}'"
        ));
    }
    let text = |key: &str| {
        table
            .get(key)
            .and_then(|value| value.as_str())
            .map(String::from)
    };
    let rules = table
        .get("rules")
        .and_then(|rules| rules.as_table())
        .into_iter()
        .flatten();
    Ok(Settings {
        dialect: text("dialect"),
        import_style: text("import-style"),
        rules: rules
            .filter_map(|(rule, severity)| Some((rule.clone(), severity.as_str()?.to_string())))
            .collect(),
    })
}

/// The row type, columns and parameters of the query `sql` on `line`.
fn described(line: usize, sql: &str, dialect: &str, options: &Options<'_>) -> Value {
    let columns = analyze_select(sql, dialect, options.schema).ok().flatten();
    let params = analyze_params(sql, dialect, options.schema).unwrap_or_default();
    json!({
        "line": line,
        "sql": sql,
        "row_type": columns.as_deref().and_then(row_type),
        "columns": columns.iter().flatten().map(|column| json!({
            "name": column.name,
            "type": column.display_type(),
            "sql_type": column.sql_type,
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
