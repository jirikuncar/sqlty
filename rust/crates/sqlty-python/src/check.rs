//! Every diagnostic of a module before inline suppression, as `sqlty._check.diagnostics`
//! reports them: the lexing pass of [`crate::checker`], then the calls that run a query
//! ([`crate::calls`]) against their parameters, names and row types ([`crate::rows`]).
//!
//! [`diagnostics`] answers `None` for a module that needs what is not read here yet.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, LazyLock};

use regex::Regex;
use ruff_python_ast::{Expr, ExprCall, Operator};
use ruff_text_size::Ranged;
use sqlty_sql::analyzer::{self, Nullability};

use crate::analyses::Analyses;
use crate::calls::{helper_calls_in, is_marked, Call};
use crate::checker::{self, Diagnostic};
use crate::facts::ModuleFacts;
use crate::frozen::Table;
use crate::index::HelperIndex;
use crate::rewriter::ImportStyle;
use crate::rows::Reported;
use crate::rows::{
    alias_target, bind, row_queries, row_shape, value_diagnostics, Check, Found, RowQuery,
    RowShape, Settings,
};
use crate::rules::{Pass, Rule, RuleConfig};
use crate::scopes::{class_of, dotted, Key, Param};
use crate::types::generic::is_word;
use crate::types::info::Kind;
use crate::types::resolve::slice_items;
use crate::types::symbols::Source;
use crate::unparse::unparse;
use crate::{drivers, module, templates};

/// A finding of the SQL linter, which the caller runs.
pub struct Lint {
    /// `CODE: what is wrong`.
    pub message: String,
    /// Byte offsets of what it underlines.
    pub span: Range<usize>,
    /// The byte offset of the query it is in.
    pub query_start: usize,
}

/// How the query passes are run, besides [`Settings`].
pub struct Run<'a> {
    pub settings: &'a Settings<'a>,
    /// Whether tables are known: a schema is set, and it is not an absent database.
    pub has_schema: bool,
    /// The SQL linter, run when `sqruff` is on.
    pub lint: &'a dyn Fn(&str) -> Vec<Lint>,
    /// The modules of the checked paths, the same as `settings.index`.
    pub helpers: Option<&'a HelperIndex>,
    /// `max-query-variants`, when queries built in branches are read.
    pub variants: Option<usize>,
}

/// Where the variants of calls are checked: variant `i` is on line `VARIANT_LINE + i`.
const VARIANT_LINE: usize = 1 << 30;
/// The types a `P` of `SQL[R, P]` takes for each parameter key, as arguments are spelled.
type Annotations = Vec<(Key, Vec<Option<String>>)>;

/// A Python `dict` built from `pairs`: each key once, where it first comes, holding its
/// last value.
fn dict<K: PartialEq, V>(pairs: impl IntoIterator<Item = (K, V)>) -> Vec<(K, V)> {
    let mut found: Vec<(K, V)> = Vec::new();
    for (key, value) in pairs {
        match found.iter_mut().find(|(known, _)| *known == key) {
            Some(known) => known.1 = value,
            None => found.push((key, value)),
        }
    }
    found
}

/// `_fits`: whether a type checker takes a value of type `actual` for `expected`.
fn fits(actual: Option<&str>, expected: Option<&str>) -> bool {
    let (Some(actual), Some(expected)) = (actual, expected) else {
        return true;
    };
    if actual == expected {
        return true;
    }
    if actual == "list" || expected == "list" {
        return actual.starts_with("list") && expected.starts_with("list");
    }
    matches!(
        (actual, expected),
        ("bool", "int" | "float") | ("int", "float") | ("datetime.datetime", "datetime.date")
    )
}

/// `_merge_variants`: the diagnostics of variants reported at their call. One every
/// variant of a call has is reported once; any other names the lines that select the
/// first variant having it.
fn merge_variants(diagnostics: Vec<Reported>, calls: &[Call<'_>]) -> Vec<Reported> {
    let mut found: HashMap<usize, Vec<Reported>> = HashMap::new();
    let mut merged = Vec::new();
    for reported in diagnostics {
        match reported.diagnostic.line.checked_sub(VARIANT_LINE) {
            Some(index) => found.entry(index).or_default().push(reported),
            None => merged.push(reported),
        }
    }
    let mut start = 0;
    for call in calls {
        let indexes = start..start + call.variants.len();
        start = indexes.end;
        let key = |reported: &Reported| {
            let diagnostic = &reported.diagnostic;
            (diagnostic.rule, diagnostic.message.clone())
        };
        let keys: Vec<HashSet<(Rule, String)>> = indexes
            .clone()
            .map(|index| found.get(&index).into_iter().flatten().map(key).collect())
            .collect();
        let common: HashSet<(Rule, String)> = keys
            .iter()
            .cloned()
            .reduce(|common, keys| common.intersection(&keys).cloned().collect())
            .unwrap_or_default();
        let mut reported: HashSet<(Rule, String)> = HashSet::new();
        for (index, variant) in indexes.zip(&call.variants) {
            for mut one in found.remove(&index).unwrap_or_default() {
                let key = key(&one);
                if !reported.insert(key.clone()) {
                    continue;
                }
                one.diagnostic.line = call.line;
                if !common.contains(&key) && !variant.lines.is_empty() {
                    let lines: Vec<String> = variant.lines.iter().map(usize::to_string).collect();
                    one.diagnostic.message += &format!(" (variant: line {})", lines.join(", "));
                }
                merged.push(one);
            }
        }
    }
    merged
}

type Span = (usize, usize);
/// `(query, line, span of the query)` of a call.
type Unread = (String, usize, Option<Span>);

/// `ParsedSource.may_need_rust_check`: whether the lexing pass can report anything.
fn may_need_lexing(text: &str, wrapper: &str) -> bool {
    // A marker split over literals: quotes, prefixes, whitespace and comments between its characters.
    static MARKER: LazyLock<Regex> = LazyLock::new(|| {
        let gap = r#"(?:[\s\x1c-\x1f"'rRuU]|#[^\n]*\n)*"#;
        sqlty_sql::regex(&format!("(?i)-{gap}-{gap}s{gap}q{gap}l"))
    });
    text.contains(&format!("{wrapper}[")) || text.contains("UNTYPED") || MARKER.is_match(text)
}

/// `ParsedSource.may_wrap`: whether the text can hold a `SQL[T](...)` call.
pub(crate) fn may_wrap(text: &str, import_line: &str) -> bool {
    let name = import_line
        .trim()
        .strip_prefix("from sqlty import SQL as ")
        .map_or("SQL", str::trim);
    text.contains(name)
}

/// `_names_params`: whether `text` subscripts `wrapper` with two arguments.
fn names_params(text: &str, wrapper: &str) -> bool {
    if !text.contains(&format!("{wrapper}[")) {
        return false;
    }
    let opening = format!("{wrapper}[");
    // `\b{wrapper}\[`, without compiling a pattern for every module.
    let starts_word = wrapper.chars().next().is_some_and(is_word);
    for (at, _) in text.match_indices(&opening) {
        if text[..at].chars().next_back().is_some_and(is_word) == starts_word {
            continue;
        }
        let mut depth = 0i32;
        for c in text[at + opening.len()..].chars() {
            match c {
                ',' if depth == 0 => return true,
                '[' | '(' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

/// A `SQL[R, P]("...")` call.
struct Declared<'a> {
    line: usize,
    params: &'a Expr,
    query: String,
    /// The byte offsets of the call.
    span: (usize, usize),
}

/// The `SQL[R, P]("...")` call `call` is, if it is one.
fn declared<'a>(source: &Source, wrapper: &str, call: &'a ExprCall) -> Option<Declared<'a>> {
    let Expr::Subscript(func) = call.func.as_ref() else {
        return None;
    };
    let Expr::Tuple(items) = func.slice.as_ref() else {
        return None;
    };
    let [_, params] = items.elts.as_slice() else {
        return None;
    };
    if dotted(&func.value).as_deref() != Some(wrapper) {
        return None;
    }
    let Some(Expr::StringLiteral(query)) = call.arguments.args.first() else {
        return None;
    };
    Some(Declared {
        line: source.line(call.start()),
        params,
        query: crate::parse::text(query, &source.text),
        span: (call.start().to_usize(), call.end().to_usize()),
    })
}

/// `_declared_params`: each `SQL[R, P]("...")` call, in source order.
fn declared_params<'a>(
    facts: &ModuleFacts<'a>,
    wrapper: &str,
    import_line: &str,
) -> Vec<Declared<'a>> {
    let source = facts.source;
    if !may_wrap(&source.text, import_line) || !names_params(&source.text, wrapper) {
        return Vec::new();
    }
    let mut found: Vec<Declared<'a>> = facts
        .calls()
        .filter_map(|call| declared(source, wrapper, call))
        .collect();
    found.sort_by_key(|call| call.span);
    found
}

/// `is_any`: whether `expr` is `typing.Any`, quoted or in `Annotated` too.
fn is_any(expr: &Expr, source: &Source) -> bool {
    let quoted;
    let expr = match expr {
        Expr::StringLiteral(text) => {
            let Some(parsed) = crate::parse::expression(text.value.to_str()) else {
                return false;
            };
            quoted = parsed;
            &*quoted.syntax().body
        }
        other => other,
    };
    match expr {
        Expr::Subscript(sub) if source.typing_name(&sub.value) == Some("Annotated") => {
            match sub.slice.as_ref() {
                Expr::Tuple(items) => items
                    .elts
                    .first()
                    .is_some_and(|first| is_any(first, source)),
                _ => false,
            }
        }
        _ => source.typing_name(expr) == Some("Any"),
    }
}

const NUMBERS: [&str; 4] = ["int", "float", "bool", "decimal.Decimal"];
const ASYNCPG_NOT_TEXT: [&str; 8] = [
    "int",
    "float",
    "bool",
    "bytes",
    "datetime.date",
    "datetime.datetime",
    "datetime.time",
    "datetime.timedelta",
];

fn accepts(expected: &str) -> Option<&'static [&'static str]> {
    Some(match expected {
        "int" | "float" | "decimal.Decimal" => &NUMBERS,
        "str" => &["str"],
        "bool" => &["bool", "int"],
        "bytes" => &["bytes"],
        "datetime.date" | "datetime.datetime" => &["datetime.date", "datetime.datetime"],
        "datetime.time" => &["datetime.time"],
        "datetime.timedelta" => &["datetime.timedelta"],
        "uuid.UUID" => &["uuid.UUID"],
        _ => return None,
    })
}

/// Why a value of type `actual` can't be passed for a parameter
/// of type `expected`.
pub fn mismatch(
    expected: Option<&str>,
    nullable: Option<bool>,
    actual: Option<&str>,
    postgres: bool,
    asyncpg: bool,
) -> Option<String> {
    let actual = actual?;
    if actual == "None" {
        return (nullable == Some(false)).then(|| "is not nullable, got None".to_string());
    }
    let expected = expected?;
    if actual == "str"
        && !(asyncpg && (ASYNCPG_NOT_TEXT.contains(&expected) || expected.starts_with("list[")))
    {
        return None;
    }
    let problem = || Some(format!("expects {expected}, got {actual}"));
    if let Some(item) = expected.strip_prefix("list[") {
        if let Some(inner) = actual.strip_prefix("list[") {
            let (item, inner) = (item.strip_suffix(']')?, inner.strip_suffix(']')?);
            return mismatch(Some(item), None, Some(inner), postgres, asyncpg)
                .and_then(|_| problem());
        }
        return if actual == "list" { None } else { problem() };
    }
    let accepted = accepts(expected)?;
    let dropped = match (postgres, expected) {
        (true, "bool") => Some("int"),
        (true, _) if !asyncpg => Some("bool"),
        _ => None,
    };
    if accepted.contains(&actual) && dropped != Some(actual) {
        return None;
    }
    problem()
}

/// `_union_members`: the members of a `X | Y`, `Optional[X]` or `Union[X, Y]` annotation;
/// `None` stands for the `None` that `Optional` adds.
fn union_members(annotation: &Expr) -> Vec<Option<&Expr>> {
    match annotation {
        Expr::BinOp(op) if op.op == Operator::BitOr => {
            let mut members = union_members(&op.left);
            members.extend(union_members(&op.right));
            members
        }
        Expr::Subscript(sub) => {
            let name = dotted(&sub.value).unwrap_or_default();
            if name.ends_with("Optional") {
                let mut members = union_members(&sub.slice);
                members.push(None);
                members
            } else if let (true, Expr::Tuple(items)) = (name.ends_with("Union"), sub.slice.as_ref())
            {
                items.elts.iter().flat_map(union_members).collect()
            } else {
                vec![Some(annotation)]
            }
        }
        _ => vec![Some(annotation)],
    }
}

/// The type a value annotated `expr` has, as argument
/// types are spelled.
fn annotation_type(expr: Option<&Expr>, imported: &Table) -> Option<String> {
    let Some(expr) = expr else {
        return Some("None".to_string());
    };
    match expr {
        Expr::NoneLiteral(_) => Some("None".to_string()),
        Expr::Subscript(sub)
            if class_of(dotted(&sub.value), imported).as_deref() == Some("list") =>
        {
            match annotation_type(Some(&sub.slice), imported) {
                Some(inner) if inner != "None" => Some(format!("list[{inner}]")),
                _ => Some("list".to_string()),
            }
        }
        _ => class_of(dotted(expr), imported),
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn nullable(nullability: Nullability) -> Option<bool> {
    match nullability {
        Nullability::Nullable => Some(true),
        Nullability::NonNull => Some(false),
        Nullability::Unknown => None,
    }
}

fn key_text(key: &Key) -> String {
    match key {
        Key::Index(index) => index.to_string(),
        Key::Name(name) => format!("'{name}'"),
    }
}

struct Module<'a, 'r> {
    run: &'r Run<'r>,
    ctx: Check<'a>,
    source: &'a Source,
    facts: &'r ModuleFacts<'a>,
    /// Helper calls.
    found: Vec<Call<'a>>,
    driver_calls: Vec<Call<'a>>,
    declared: Vec<Declared<'a>>,
    /// The row type of each helper call with a query, as written.
    rows: Vec<Option<String>>,
    literals: Option<HashSet<String>>,
}

impl<'a> Module<'a, '_> {
    fn dialect(&self) -> &'static str {
        self.run.settings.options.dialect.name()
    }

    fn enabled(&self, rule: Rule) -> bool {
        self.run.settings.rules.is_enabled(rule)
    }

    fn params(&self, query: &str) -> Option<Vec<analyzer::Param>> {
        self.ctx.analyses.params(query)
    }

    /// The helper calls with a query, each with its row type as written.
    fn calls(&self) -> impl Iterator<Item = (&Call<'a>, Option<&String>)> {
        self.found
            .iter()
            .filter(|call| call.query.is_some())
            .zip(&self.rows)
            .map(|(call, row)| (call, row.as_ref()))
    }

    /// Whether the engine reads `query` by its marker: a marked literal of the module.
    fn read_by_marker(&mut self, query: &str) -> bool {
        let facts = self.facts;
        is_marked(query)
            && self
                .literals
                .get_or_insert_with(|| facts.literals())
                .contains(query)
    }

    fn unchecked_queries(&mut self) {
        let mut reports = Vec::new();
        for call in &self.found {
            if call.query.is_some() || call.row.is_some_and(|row| is_any(row, self.source)) {
                continue;
            }
            let (rule, message) = match (&call.reason, call.dynamic) {
                (Some(reason), dynamic) => (
                    if dynamic {
                        Rule::DynamicQuery
                    } else {
                        Rule::UncheckedQuery
                    },
                    format!("query has {reason}, so it is not checked"),
                ),
                (None, true) => (
                    Rule::DynamicQuery,
                    "query is not a string literal, so it is not checked".to_string(),
                ),
                (None, false) => (
                    Rule::UncheckedQuery,
                    "psycopg query is not built from literals only, so it is not checked"
                        .to_string(),
                ),
            };
            reports.push((rule, message, call.line, call.query_span));
        }
        for (rule, message, line, span) in reports {
            self.ctx.report(rule, message, line, span);
        }
    }

    fn check_params(&mut self) {
        let mut reports = Vec::new();
        if self.enabled(Rule::ParamTypeMismatch) {
            let mut singles: Vec<(usize, &str, &str, Option<Span>)> = Vec::new();
            for call in &self.driver_calls {
                let (Some(single), Some(driver)) = (call.single.as_deref(), call.driver) else {
                    continue;
                };
                let key = (call.line, single, driver);
                match singles.iter_mut().find(|(l, s, d, _)| (*l, *s, *d) == key) {
                    Some(known) => known.3 = call.query_span,
                    None => singles.push((call.line, single, driver, call.query_span)),
                }
            }
            singles.sort_by_key(|(line, single, driver, _)| (*line, *single, *driver));
            for (line, single, driver, span) in singles {
                reports.push((
                    Rule::ParamTypeMismatch,
                    format!(
                        "parameters are a single {single}, which {driver} rejects: pass a sequence, `(value,)`"
                    ),
                    line,
                    span,
                ));
            }
        }
        let calls = self.calls().map(|(call, _)| call).chain(&self.driver_calls);
        for call in calls {
            if let (Some(query), Some(params)) = (&call.query, &call.params) {
                let declared = call
                    .declared
                    .as_ref()
                    .and_then(|declared| self.annotations_of(declared.params?, declared.source));
                for (rule, message) in
                    self.param_diagnostics(call, query, params, declared.as_ref())
                {
                    reports.push((rule, message, call.line, call.query_span));
                }
            }
        }
        for (rule, message, line, span) in reports {
            self.ctx.report(rule, message, line, span);
        }
    }

    /// `_param_diagnostics`: the arguments a call passes for the placeholders of its query.
    fn param_diagnostics(
        &self,
        call: &Call<'_>,
        query: &str,
        passed: &[Param],
        declared: Option<&Annotations>,
    ) -> Vec<(Rule, String)> {
        let Some(params) = self.params(query) else {
            return Vec::new();
        };
        let named = !params.is_empty() && params.iter().all(|p| p.name.is_some());
        let positional = params.iter().all(|p| p.name.is_none());
        // A key passed twice counts once, where it first is, with its last value.
        let mut args: Vec<&Param> = Vec::new();
        for arg in passed {
            match args.iter_mut().find(|known| known.key == arg.key) {
                Some(known) => *known = arg,
                None => args.push(arg),
            }
        }
        let key_of = |param: &analyzer::Param| match &param.name {
            Some(name) if named && !name.is_empty() => Key::Name(name.clone()),
            _ => Key::Index(param.index),
        };
        let by_key = dict(params.iter().map(|param| (key_of(param), param)));
        let find = |key: &Key| {
            by_key
                .iter()
                .find(|(known, _)| known == key)
                .map(|(_, p)| *p)
        };
        let by_name = !args.is_empty() && args.iter().all(|arg| matches!(arg.key, Key::Name(_)));
        if (named && !args.is_empty() && !by_name) || (positional && by_name && !params.is_empty())
        {
            return Vec::new();
        }
        let mut found = Vec::new();
        if by_name {
            let extra: Vec<String> = args
                .iter()
                .filter(|arg| find(&arg.key).is_none())
                .map(|arg| key_text(&arg.key))
                .collect();
            if !extra.is_empty() {
                found.push((
                    Rule::ExtraParam,
                    format!(
                        "call passes extra parameter{} {}",
                        plural(extra.len()),
                        extra.join(", ")
                    ),
                ));
            }
        }
        if positional && !by_name && args.len() != by_key.len() {
            found.push((
                Rule::ParamCountMismatch,
                format!(
                    "query has {} parameter{}, the call passes {}",
                    by_key.len(),
                    plural(by_key.len()),
                    args.len()
                ),
            ));
        } else if named {
            let missing: Vec<String> = by_key
                .iter()
                .filter(|(key, _)| !args.iter().any(|arg| arg.key == *key))
                .map(|(key, _)| key_text(key))
                .collect();
            if !missing.is_empty() {
                found.push((
                    Rule::ParamCountMismatch,
                    format!(
                        "call is missing parameter{} {}",
                        plural(missing.len()),
                        missing.join(", ")
                    ),
                ));
            }
        }
        // sqlite3 is dynamically typed, and a cast converts whatever it is given.
        let dialect = self.dialect().to_lowercase();
        let typed = dialect != "sqlite";
        let postgres = dialect == "postgres" || dialect == "postgresql";
        let asyncpg = call.driver == Some("asyncpg");
        for arg in &args {
            let Some(param) = find(&arg.key) else {
                continue;
            };
            let label = match &arg.key {
                Key::Index(index) => format!("parameter {index}"),
                Key::Name(name) => format!("parameter '{name}'"),
            };
            let annotated = declared
                .and_then(|declared| declared.iter().find(|(key, _)| *key == arg.key))
                .map(|(_, members)| members);
            if let Some(members) = annotated {
                let kind = arg.kind.as_deref();
                if !members.iter().any(|member| fits(kind, member.as_deref())) {
                    let spelled: Vec<&str> = members
                        .iter()
                        .map(|member| member.as_deref().unwrap_or("None"))
                        .collect();
                    found.push((
                        Rule::ParamTypeMismatch,
                        format!(
                            "{label} expects {}, got {}",
                            spelled.join(" | "),
                            kind.unwrap_or("None")
                        ),
                    ));
                }
            } else if arg.kind.as_deref() == Some("None") || (typed && !param.cast) {
                let mut expected = param.python_type.as_deref();
                if expected.is_none()
                    && asyncpg
                    && param.untyped_sql_type.as_deref() == Some("INTERVAL")
                {
                    expected = Some("datetime.timedelta");
                }
                if let Some(problem) = mismatch(
                    expected,
                    nullable(param.nullability),
                    arg.kind.as_deref(),
                    postgres,
                    asyncpg,
                ) {
                    found.push((Rule::ParamTypeMismatch, format!("{label} {problem}")));
                }
            }
        }
        if asyncpg {
            for param in &params {
                if let (Some(untyped), Some(sql_type)) = (
                    param.untyped_sql_type.as_deref().filter(|t| !t.is_empty()),
                    param.sql_type.as_deref().filter(|t| !t.is_empty()),
                ) {
                    let (untyped, sql_type) = (untyped.to_lowercase(), sql_type.to_lowercase());
                    let number = param.index;
                    found.push((
                        Rule::ParamTypeMismatch,
                        format!(
                            "parameter {number} is an `{untyped}` to PostgreSQL, as asyncpg sends it \
                             untyped, so it cannot stand for a `{sql_type}`: \
                             cast it, `${number}::{sql_type}`"
                        ),
                    ));
                }
            }
        }
        found
    }

    /// `SQL[R, P]` calls whose `P` doesn't fit the parameters of their query.
    fn declared_params(&mut self) {
        let mut reports = Vec::new();
        for call in &self.declared {
            let Some(params) = self.params(&call.query) else {
                continue;
            };
            let span = (call.params.start().to_usize(), call.params.end().to_usize());
            for message in self.declared_params_diagnostics(call.params, &params) {
                reports.push((message, call.line, span));
            }
        }
        for (message, line, span) in reports {
            self.ctx
                .report(Rule::ParamsTypeMismatch, message, line, Some(span));
        }
    }

    /// `_declared_params_diagnostics`: why the parameter type `declared` doesn't fit
    /// `params`: shape, count, keys, then item types.
    fn declared_params_diagnostics(
        &self,
        declared: &Expr,
        params: &[analyzer::Param],
    ) -> Vec<String> {
        let text = &self.source.text;
        let name = unparse(declared, text);
        let named = !params.is_empty() && params.iter().all(|p| p.name.is_some());
        let dotted_name = dotted(declared).unwrap_or_default();
        let count = |n: usize| format!("{n} parameter{}", plural(n));
        // `(label, annotation, parameter, key)`
        let mut pairs: Vec<(String, &Expr, &analyzer::Param, String)> = Vec::new();
        let info = self.ctx.type_defs.get(&name);
        if dotted_name.rsplit('.').next() == Some("NoParams") && info.is_none() {
            return if params.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "{name} takes none but the query has {}",
                    count(params.len())
                )]
            };
        }
        let tuple = match declared {
            Expr::Subscript(sub)
                if matches!(
                    dotted(&sub.value).as_deref(),
                    Some("tuple" | "typing.Tuple")
                ) =>
            {
                Some(slice_items(&sub.slice))
            }
            _ => None,
        };
        if let Some(items) = tuple {
            if items.iter().any(Expr::is_ellipsis_literal_expr) {
                return Vec::new();
            }
            if named {
                return vec![format!(
                    "{name} is a tuple but the query takes named parameters"
                )];
            }
            if items.len() != params.len() {
                return vec![format!(
                    "{name} has {} item{} but the query has {}",
                    items.len(),
                    plural(items.len()),
                    count(params.len())
                )];
            }
            for (item, param) in items.iter().zip(params) {
                let number = param.index;
                pairs.push((
                    format!("{name} item {number}"),
                    item,
                    param,
                    number.to_string(),
                ));
            }
        } else if let Some(info) = info.filter(|info| info.kind == Kind::TypedDict) {
            if !params.is_empty() && !named {
                return vec![format!(
                    "{name} is a TypedDict but the query takes positional parameters"
                )];
            }
            let keys: Vec<&str> = info.fields.iter().map(|(key, _)| key.as_str()).collect();
            let by_name = dict(
                params
                    .iter()
                    .filter_map(|param| Some((param.name.as_deref()?, param))),
            );
            let mut found: Vec<String> = by_name
                .iter()
                .filter(|(key, _)| !keys.contains(key))
                .map(|(key, _)| format!("{name} has no key for query parameter '{key}'"))
                .collect();
            found.extend(
                by_name
                    .iter()
                    .filter(|(key, _)| keys.contains(key) && !info.required.contains(*key))
                    .map(|(key, _)| {
                        format!("{name}.{key} is not required but query parameter '{key}' is")
                    }),
            );
            found.extend(
                keys.iter()
                    .filter(|key| !by_name.iter().any(|(known, _)| known == *key))
                    .map(|key| format!("{name}.{key} is not a query parameter")),
            );
            if !found.is_empty() {
                return found;
            }
            let in_key_order = keys.iter().filter_map(|key| {
                let (_, param) = by_name.iter().find(|(known, _)| known == key)?;
                Some((key, *param, &info.annotations.get(key)?.expr))
            });
            for (key, param, annotation) in in_key_order {
                pairs.push((
                    format!("{name}.{key}"),
                    annotation,
                    param,
                    format!("'{key}'"),
                ));
            }
        } else {
            return Vec::new();
        }
        let dialect = self.dialect().to_lowercase();
        let typed = dialect != "sqlite";
        let postgres = dialect == "postgres" || dialect == "postgresql";
        let mut found = Vec::new();
        for (label, annotation, param, key) in pairs {
            for member in union_members(annotation) {
                let actual = annotation_type(member, &self.source.imports);
                if actual.as_deref() != Some("None") && !(typed && !param.cast) {
                    continue;
                }
                if let Some(problem) = mismatch(
                    param.python_type.as_deref(),
                    nullable(param.nullability),
                    actual.as_deref(),
                    postgres,
                    false,
                ) {
                    let expected = problem.split(", got ").next().unwrap_or(&problem);
                    // The annotation of a key of a TypedDict is of the source that defines it.
                    let spelled = match info.and_then(|info| {
                        info.annotations
                            .values()
                            .find(|a| std::ptr::eq(&a.expr, annotation))
                    }) {
                        Some(annotation) => annotation.unparse(),
                        None => unparse(annotation, text),
                    };
                    found.push(format!(
                        "{label} is {spelled} but query parameter {key} {expected}"
                    ));
                    break;
                }
            }
        }
        found
    }

    fn select_star(&mut self) {
        let mut reports = Vec::new();
        for (call, _) in self.calls() {
            let Some(query) = call.query.as_deref().filter(|query| !query.is_empty()) else {
                continue;
            };
            let stars = self.ctx.analyses.of(query).parsed.result_stars();
            if let Some((rule, message)) = checker::star_finding(stars, query, self.run.has_schema)
            {
                reports.push((rule, message, call.line, call.query_span));
            }
        }
        for (rule, message, line, span) in reports {
            self.ctx.report(rule, message, line, span);
        }
    }

    /// The helper calls whose query the engine does not read by its marker.
    fn unread(&mut self) -> Vec<Unread> {
        let calls: Vec<Unread> = self
            .calls()
            .filter_map(|(call, _)| Some((call.query.clone()?, call.line, call.query_span)))
            .collect();
        calls
            .into_iter()
            .filter(|(query, _, _)| !self.read_by_marker(query))
            .collect()
    }

    fn unparsed(&mut self) {
        static FORMAT_FIELD: LazyLock<Regex> = LazyLock::new(|| sqlty_sql::regex(r"\{\w*\}"));
        for (query, line, span) in self.unread() {
            let Some(error) = self.ctx.analyses.of(&query).parsed.error() else {
                continue;
            };
            let mut message = format!("query does not parse: {error}");
            if let Some(field) = FORMAT_FIELD
                .find(&query)
                .filter(|_| error.contains("found: {"))
            {
                message.push_str(&format!(
                    "; `{}` is a `.format()` field: fill it with `sql.SQL(...).format(...)`, or pass a value as `%s`",
                    field.as_str()
                ));
            }
            self.ctx.report(Rule::UnparsedQuery, message, line, span);
        }
    }

    fn names(&mut self) {
        for (query, line, span) in self.unread() {
            let analysis = self.ctx.analyses.of(&query);
            for diagnostic in &analysis.diagnostics {
                self.ctx
                    .report(diagnostic.rule, diagnostic.message.clone(), line, span);
            }
        }
    }

    /// `_annotations_of`: `P` of a `SQL[R, P]` in `source`, as the types each parameter
    /// key takes.
    fn annotations_of(&self, declared: &Expr, source: &Source) -> Option<Annotations> {
        let types = |annotation: &Expr| -> Vec<Option<String>> {
            union_members(annotation)
                .into_iter()
                .map(|member| annotation_type(member, &source.imports))
                .collect()
        };
        let tuple = declared.as_subscript_expr().filter(|sub| {
            matches!(
                dotted(&sub.value).as_deref(),
                Some("tuple" | "typing.Tuple")
            )
        });
        if let Some(sub) = tuple {
            let items = slice_items(&sub.slice);
            if items.iter().any(Expr::is_ellipsis_literal_expr) {
                return None;
            }
            return Some(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| (Key::Index(i + 1), types(item)))
                    .collect(),
            );
        }
        let name = unparse(declared, &source.text);
        let own = if std::ptr::eq(source, self.source) {
            self.ctx.own_defs.get(&name).cloned()
        } else {
            let defs = self.run.helpers?.definitions(source)?;
            defs.get(&name).cloned()
        };
        let info = own.or_else(|| {
            let index = self.run.settings.index?;
            Some(index.row_type(source, &name)?.info)
        })?;
        (info.kind == Kind::TypedDict).then(|| {
            info.annotations
                .iter()
                .map(|(key, annotation)| (Key::Name(key.to_string()), types(&annotation.expr)))
                .collect()
        })
    }

    /// `_bound_params`: the arguments a `SQL[R](...)` query is bound to, by the span of `R`.
    fn bound_params(&self) -> HashMap<(usize, usize), Vec<Param>> {
        let mut bindings: Vec<((usize, usize), Vec<Param>)> = self
            .driver_calls
            .iter()
            .filter_map(|call| {
                let params = call.params.as_ref().filter(|params| !params.is_empty())?;
                Some((call.query_span?, params.clone()))
            })
            .collect();
        for call in &self.declared {
            let Some(annotations) = self.annotations_of(call.params, self.source) else {
                continue;
            };
            let params = annotations
                .into_iter()
                .map(|(key, types)| {
                    let types: HashSet<Option<String>> = types.into_iter().collect();
                    let kind = match types.len() {
                        1 => types.into_iter().next().flatten(),
                        _ => None,
                    };
                    Param { key, kind }
                })
                .collect();
            bindings.push((call.span, params));
        }
        let mut bound = HashMap::new();
        for (_, _, _, row) in &self.ctx.user_queries {
            for ((start, end), params) in &bindings {
                if *start <= row.0 && row.1 <= *end {
                    bound.insert(*row, params.clone());
                }
            }
        }
        bound
    }

    /// Rows spelled as a tuple, `list[...]` etc. rather than a named type.
    fn values(&mut self) {
        let aliases = self.source.type_aliases();
        let mut shapes: HashMap<String, Option<RowShape>> = HashMap::new();
        let mut reports = Vec::new();
        for (call, row) in self.calls() {
            let (Some(node), Some(row)) = (call.row, row) else {
                continue;
            };
            if self.ctx.type_defs.contains(row)
                || alias_target(row, &aliases, self.source).is_some()
            {
                continue;
            }
            let query = bind(
                call.query.as_deref(),
                call.params.as_deref(),
                self.dialect(),
            );
            let Some(columns) = self.ctx.columns(query.as_deref()) else {
                continue;
            };
            let shape = shapes
                .entry(row.clone())
                .or_insert_with(|| row_shape(node, row, self.source));
            let span = (node.start().to_usize(), node.end().to_usize());
            let rules = self.run.settings.rules;
            for (rule, message) in
                value_diagnostics(shape.as_ref(), &columns, rules, &self.ctx.type_defs)
            {
                reports.push((rule, message, call.line, span));
            }
        }
        for (rule, message, line, span) in reports {
            self.ctx.report(rule, message, line, Some(span));
        }
    }
}

/// A helper call as `fix_source`, `query_statistics` and the highlights read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// The first and last line of a call whose strings need no `SQL[T]`.
    pub typed: Option<(usize, usize)>,
    /// The queries it can run that the lexer does not read by their marker.
    pub queries: Vec<String>,
    /// The rule of a call that is not checked: `dynamic-query` or `unchecked-query`.
    pub unchecked: Option<Rule>,
    /// The byte offset of the string literal it passes as its query.
    pub start: Option<usize>,
}

/// The helper calls of `source`, by line.
pub fn sites(
    source: &Source,
    helpers: Option<&HelperIndex>,
    module: Option<&str>,
    variants: Option<usize>,
) -> Vec<Site> {
    let facts = ModuleFacts::new(source);
    let calls = helper_calls_in(&facts, helpers, module, variants);
    let mut read: Option<HashSet<String>> = None;
    let mut by_marker = |query: &str| {
        is_marked(query) && read.get_or_insert_with(|| facts.literals()).contains(query)
    };
    calls
        .into_iter()
        .map(|call| {
            let lines = call.typed_lines();
            let queries: Vec<String> = match (&call.query, call.variants.is_empty()) {
                (None, _) => Vec::new(),
                (Some(query), true) => vec![query.clone()],
                (Some(_), false) => call.variants.iter().map(|v| v.query.clone()).collect(),
            };
            Site {
                typed: (!lines.is_empty()).then(|| (*lines.start(), *lines.end())),
                unchecked: (call.query.is_none()
                    && !call.row.is_some_and(|row| is_any(row, source)))
                .then_some(if call.dynamic {
                    Rule::DynamicQuery
                } else {
                    Rule::UncheckedQuery
                }),
                queries: queries.into_iter().filter(|q| !by_marker(q)).collect(),
                start: call.query_span.filter(|_| call.literal).map(|span| span.0),
            }
        })
        .collect()
}

/// [`diagnostics`] of `parsed`, else [`unparsed`] of `text`; a panic becomes one
/// `internal-error` in their place, so one module cannot stop a whole check.
pub fn module(text: &str, parsed: Option<&Arc<Source>>, run: &Run<'_>) -> Found {
    catch_unwind(AssertUnwindSafe(|| match parsed {
        Some(parsed) => diagnostics(parsed, run),
        None => unparsed(text, run),
    }))
    .unwrap_or_else(|payload| crashed(crate::report::cause(&*payload), run.settings.rules))
}

/// The `internal-error` of a check that panicked with `cause`.
pub fn crashed(cause: &str, rules: &RuleConfig) -> Found {
    let diagnostic = Diagnostic {
        rule: Rule::InternalError,
        message: format!(
            "sqlty crashed checking this file: {cause}; please report it at {}",
            crate::report::issue_url(cause)
        ),
        severity: rules.severity(Rule::InternalError),
        line: 1,
        span: 0..0,
        fix: Vec::new(),
    };
    Found {
        diagnostics: rules
            .is_enabled(Rule::InternalError)
            .then_some(Reported {
                diagnostic,
                redundant: None,
            })
            .into_iter()
            .collect(),
        needed: HashSet::new(),
    }
}

/// The diagnostics of a source that does not parse (see [`crate::parse`]): what its
/// strings alone tell, by line.
pub fn unparsed(text: &str, run: &Run<'_>) -> Found {
    let (options, rules) = (run.settings.options, run.settings.rules);
    let mut found = Vec::new();
    if may_need_lexing(text, &options.style.wrapper_name) {
        found = checker::check_source(text, options, rules, &[]);
        checker::add_fix_imports(text, options, &mut found);
    }
    if rules.is_enabled(Rule::Sqruff) {
        let (severity, lines) = (
            rules.severity(Rule::Sqruff),
            crate::scopes::Lines::new(text),
        );
        found.extend((run.lint)(text).into_iter().map(|lint| Diagnostic {
            rule: Rule::Sqruff,
            message: lint.message,
            severity,
            line: lines.line(lint.query_start),
            span: lint.span,
            fix: Vec::new(),
        }));
    }
    found.sort_by_key(|diagnostic| diagnostic.line);
    Found {
        diagnostics: found
            .into_iter()
            .map(|diagnostic| Reported {
                diagnostic,
                redundant: None,
            })
            .collect(),
        needed: HashSet::new(),
    }
}

/// The byte offset of each `SQL[T](...)` call of `source` whose `T` is `typing.Any`.
pub fn any_calls(source: &Source, style: &ImportStyle) -> Vec<usize> {
    if !may_have_any_calls(&source.text, style) {
        return Vec::new();
    }
    named_as_wrapper(
        module::any_calls(source.body(), &source.aliases),
        &style.wrapper_name,
    )
}

/// [`any_calls`] over a walk of the module that other checks share.
fn any_calls_in(facts: &ModuleFacts<'_>, style: &ImportStyle) -> Vec<usize> {
    let source = facts.source;
    if !may_have_any_calls(&source.text, style) {
        return Vec::new();
    }
    named_as_wrapper(
        module::any_calls_among(facts.calls(), &source.aliases),
        &style.wrapper_name,
    )
}

fn may_have_any_calls(text: &str, style: &ImportStyle) -> bool {
    text.contains("Any") && may_wrap(text, &style.import_line)
}

fn named_as_wrapper(calls: Vec<(usize, String)>, wrapper: &str) -> Vec<usize> {
    calls
        .into_iter()
        .filter(|(_, name)| {
            name == wrapper
                || name
                    .strip_suffix(wrapper)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
        .map(|(offset, _)| offset)
        .collect()
}

/// The calls of a database driver (`cursor.execute(...)` and the like) in `source`.
fn driver_calls<'a>(text: &str, source: &'a Source, facts: &ModuleFacts<'a>) -> Vec<Call<'a>> {
    if !drivers::may_hold(text) {
        return Vec::new();
    }
    drivers::find_in(text, source.body(), &source.imports, facts.scan())
        .into_iter()
        .map(|call| Call {
            line: call.line,
            query: Some(call.query),
            row: None,
            params: call.params,
            end_line: None,
            driver: Some(call.driver),
            single: call.single,
            dynamic: false,
            reason: None,
            variants: Vec::new(),
            declared: None,
            configured: false,
            query_span: Some(call.query_span),
            literal: false,
        })
        .collect()
}

/// The marked t-strings of `source`, each as a call of psycopg.
fn template_calls<'a>(text: &str, source: &Source, facts: &ModuleFacts<'a>) -> Vec<Call<'a>> {
    static MARKED_TEMPLATE: LazyLock<Regex> = LazyLock::new(|| {
        sqlty_sql::regex(
            r#"(?:^|[^\w])(?:[tT][rR]?|[rR][tT])(?:'|")+[\s\x1c-\x1f]*--[\s\x1c-\x1f]?sql"#,
        )
    });
    if !MARKED_TEMPLATE.is_match(text) {
        return Vec::new();
    }
    let mut calls = Vec::new();
    for template in templates::of_in(text, &source.imports, facts.scan()) {
        let Some(query) = template.query.filter(|query| is_marked(query)) else {
            continue;
        };
        let params = template.params.into_iter().enumerate();
        calls.push(Call {
            line: template.line,
            query: Some(query),
            row: None,
            params: Some(
                params
                    .map(|(i, kind)| Param {
                        key: Key::Index(i + 1),
                        kind,
                    })
                    .collect(),
            ),
            end_line: None,
            driver: Some("psycopg"),
            single: None,
            dynamic: false,
            reason: None,
            variants: Vec::new(),
            declared: None,
            configured: false,
            query_span: Some(template.span),
            literal: false,
        });
    }
    calls
}

/// A driver call also configured as a helper has its parameters checked once, as the driver's.
fn drop_params_checked_by_driver(found: &mut [Call<'_>], driver_calls: &[Call<'_>]) {
    let by_driver: HashSet<(usize, usize)> = driver_calls
        .iter()
        .filter_map(|call| call.query_span)
        .collect();
    for call in found {
        if call
            .query_span
            .is_some_and(|span| by_driver.contains(&span))
        {
            call.params = None;
        }
    }
}

/// The lines of the calls whose strings need no `SQL[T]`.
fn typed_lines(found: &[Call<'_>]) -> HashSet<usize> {
    found.iter().flat_map(Call::typed_lines).collect()
}

/// The calls built in branches, and `found` with each variant of them as a call of its own:
/// variant `i` is on line `VARIANT_LINE + i`.
fn expand_variants<'a>(found: Vec<Call<'a>>) -> (Vec<Call<'a>>, Vec<Call<'a>>) {
    let (branched, mut found): (Vec<Call<'a>>, Vec<Call<'a>>) =
        found.into_iter().partition(|call| call.variants.len() > 1);
    let variants = branched.iter().flat_map(|call| {
        call.variants.iter().map(move |variant| Call {
            query: Some(variant.query.clone()),
            declared: variant.declared.clone(),
            variants: Vec::new(),
            ..call.clone()
        })
    });
    let variants: Vec<Call<'a>> = variants
        .enumerate()
        .map(|(index, call)| Call {
            line: VARIANT_LINE + index,
            ..call
        })
        .collect();
    found.extend(variants);
    (branched, found)
}

/// A `stale-rewrite` on a line of a typed call is not one: the call is wrapped already.
fn drop_stale_rewrite_on_typed_lines(base: &mut Vec<Diagnostic>, typed: &HashSet<usize>) {
    base.retain(|diagnostic| {
        !(diagnostic.rule == Rule::StaleRewrite && typed.contains(&diagnostic.line))
    });
}

/// What the passes of [`diagnostics`] work on.
struct Pipeline<'a, 'r: 'a> {
    run: &'r Run<'r>,
    source: &'a Arc<Source>,
    facts: &'r ModuleFacts<'a>,
    analyses: &'a Analyses<'a>,
    /// What the passes before the call passes found.
    base: Vec<Diagnostic>,
    /// Whether the transformations of the facts are made.
    transformed: bool,
    /// The lines of typed calls.
    typed: HashSet<usize>,
    /// The calls built in branches, whose variants are checked as calls of their own.
    branched: Vec<Call<'a>>,
    /// The helper calls and driver calls, until the first pass that reads them.
    calls: Option<(Vec<Call<'a>>, Vec<Call<'a>>)>,
    /// The call facts, unless the module has no query to check.
    module: Option<Module<'a, 'r>>,
}

#[cfg(test)]
thread_local! {
    /// The passes [`Pipeline::run`] was entered for.
    static ENTERED: std::cell::RefCell<Vec<Pass>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl<'a, 'r: 'a> Pipeline<'a, 'r> {
    /// Run `pass`: the one place that says what each pass does.
    fn run(&mut self, pass: Pass) {
        #[cfg(test)]
        ENTERED.with(|entered| entered.borrow_mut().push(pass));
        match pass {
            Pass::Lexer => self.lexer(),
            Pass::Sqruff => self.sqruff(),
            Pass::Unchecked => self.with_module(pass, Module::unchecked_queries),
            Pass::Params => self.with_module(pass, Module::check_params),
            Pass::DeclaredParams => self.with_module(pass, Module::declared_params),
            Pass::TypeddictInheritance => {
                self.with_module(pass, |module| module.ctx.typeddict_inheritance());
            }
            Pass::SelectStar => self.with_module(pass, Module::select_star),
            Pass::Unparsed => self.with_module(pass, Module::unparsed),
            Pass::Names => self.with_module(pass, Module::names),
            Pass::Structural => self.with_module(pass, |module| {
                let bound = module.bound_params();
                module.ctx.structural(&bound);
            }),
            Pass::Aliases => self.with_module(pass, |module| module.ctx.aliases()),
            Pass::Values => self.with_module(pass, Module::values),
            Pass::Suppress | Pass::Crash | Pass::QueryFiles => {}
        }
    }

    /// The calls of the module, transformed: made once, after the lexer is done with its own
    /// memory.
    fn transform(&mut self) {
        if std::mem::replace(&mut self.transformed, true) {
            return;
        }
        let (run, facts) = (self.run, self.facts);
        let source = facts.source;
        let text = &*source.text;
        let mut found = helper_calls_in(facts, run.helpers, run.settings.module, run.variants);
        let mut driver_calls = driver_calls(text, source, facts);
        driver_calls.extend(template_calls(text, source, facts));
        drop_params_checked_by_driver(&mut found, &driver_calls);
        self.typed = typed_lines(&found);
        let (branched, found) = expand_variants(found);
        self.branched = branched;
        self.calls = Some((found, driver_calls));
    }

    /// The call facts, made when the first pass asks.
    fn module(&mut self) -> Option<&mut Module<'a, 'r>> {
        self.transform();
        if let Some((found, driver_calls)) = self.calls.take() {
            self.module = call_facts(
                self.run,
                self.source,
                self.facts,
                self.analyses,
                found,
                driver_calls,
            );
        }
        self.module.as_mut()
    }

    fn with_module(&mut self, pass: Pass, run: impl FnOnce(&mut Module<'a, 'r>)) {
        if let Some(module) = self.module() {
            module.ctx.pass = pass;
            run(module);
        }
    }

    fn lexer(&mut self) {
        let settings = self.run.settings;
        let (options, rules) = (settings.options, settings.rules);
        let text = &*self.facts.source.text;
        if may_need_lexing(text, options.style.wrapper_name.as_str()) {
            let any_calls = any_calls_in(self.facts, &options.style);
            self.base = checker::check_source_with(text, options, rules, &any_calls, self.analyses);
            checker::add_fix_imports(text, options, &mut self.base);
        }
        self.transform();
        drop_stale_rewrite_on_typed_lines(&mut self.base, &self.typed);
    }

    fn sqruff(&mut self) {
        let source = self.facts.source;
        let rules = self.run.settings.rules;
        let severity = rules.severity(Rule::Sqruff);
        self.base.extend(
            (self.run.lint)(&source.text)
                .into_iter()
                .map(|lint| Diagnostic {
                    rule: Rule::Sqruff,
                    message: lint.message,
                    severity,
                    line: source.lines.line(lint.query_start),
                    span: lint.span,
                    fix: Vec::new(),
                }),
        );
    }
}

/// The call facts of `source` for the passes that check queries, `None` when it holds no
/// query, call or declared parameter to check.
fn call_facts<'a, 'r: 'a>(
    run: &'r Run<'r>,
    source: &'a Arc<Source>,
    facts: &'r ModuleFacts<'a>,
    analyses: &'a Analyses<'a>,
    found: Vec<Call<'a>>,
    driver_calls: Vec<Call<'a>>,
) -> Option<Module<'a, 'r>> {
    let settings = run.settings;
    let text = &*source.text;
    let wrapper = settings.options.style.wrapper_name.as_str();
    let import_line = settings.options.style.import_line.as_str();
    let queries: Vec<RowQuery> = if may_wrap(text, import_line) {
        row_queries(source, settings)
    } else {
        Vec::new()
    };
    let declared = declared_params(facts, wrapper, import_line);
    if queries.is_empty() && found.is_empty() && driver_calls.is_empty() && declared.is_empty() {
        return None;
    }

    let dialect = settings.options.dialect.name();
    let rows: Vec<Option<String>> = found
        .iter()
        .filter(|call| call.query.is_some())
        .map(|call| call.row.map(|row| unparse(row, text)))
        .collect();
    let named: Vec<RowQuery> = found
        .iter()
        .filter(|call| call.query.is_some())
        .zip(&rows)
        .filter_map(|(call, row)| {
            let (node, row) = (call.row?, row.as_ref().filter(|row| !row.is_empty())?);
            Some((
                call.line,
                row.clone(),
                bind(call.query.as_deref(), call.params.as_deref(), dialect),
                (node.start().to_usize(), node.end().to_usize()),
            ))
        })
        .collect();
    let names: Vec<String> = declared
        .iter()
        .filter(|call| matches!(call.params, Expr::Name(_) | Expr::Attribute(_)))
        .map(|call| unparse(call.params, text))
        .collect();
    Some(Module {
        run,
        ctx: Check::new(source, settings, queries, named, &names, analyses),
        source,
        facts,
        found,
        driver_calls,
        declared,
        rows,
        literals: None,
    })
}

/// `sqlty._check.diagnostics`: every diagnostic of `source` before inline suppression, by
/// line: the facts of the module, the named transformations of them, then the passes of
/// [`Pass::PIPELINE`].
pub fn diagnostics(source: &Arc<Source>, run: &Run<'_>) -> Found {
    pipeline(source, run, Pass::PIPELINE, true)
}

/// [`diagnostics`] over `passes`; with `gated` false, the passes that rules switch off run
/// all the same.
fn pipeline(source: &Arc<Source>, run: &Run<'_>, passes: &[Pass], gated: bool) -> Found {
    let settings = run.settings;
    let analyses = Analyses::new(settings.options);
    let facts = ModuleFacts::new(source);
    let mut pipeline = Pipeline {
        run,
        source,
        facts: &facts,
        analyses: &analyses,
        base: Vec::new(),
        transformed: false,
        typed: HashSet::new(),
        branched: Vec::new(),
        calls: None,
        module: None,
    };
    for &pass in passes {
        if gated && !settings.rules.runs(pass) {
            continue;
        }
        pipeline.run(pass);
    }
    pipeline.module();
    let Pipeline {
        base,
        branched,
        module,
        ..
    } = pipeline;
    let mut out = Found {
        diagnostics: base
            .into_iter()
            .map(|diagnostic| Reported {
                diagnostic,
                redundant: None,
            })
            .collect(),
        needed: HashSet::new(),
    };
    let Some(module) = module else {
        return out;
    };
    let passes = module.ctx.finish();
    out.diagnostics.extend(passes.diagnostics);
    if !branched.is_empty() {
        out.diagnostics = merge_variants(out.diagnostics, &branched);
    }
    out.needed = passes.needed;
    out.diagnostics
        .sort_by_key(|reported| reported.diagnostic.line);
    out
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::analyses::{ANALYZED, PARSED};
    use crate::options::Options;

    fn crash(text: &str, rules: &[(&str, &str)]) -> Vec<Diagnostic> {
        run_module(text, rules, false)
            .diagnostics
            .into_iter()
            .map(|reported| reported.diagnostic)
            .collect()
    }

    fn run_module(text: &str, rules: &[(&str, &str)], parsed: bool) -> Found {
        let options = Options::new("postgres", None, None, false).unwrap();
        let rules = RuleConfig::from_overrides(Some(
            rules
                .iter()
                .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
                .collect(),
        ))
        .unwrap();
        let coerce = HashSet::new();
        let settings = Settings {
            options: &options,
            rules: &rules,
            index: None,
            module: None,
            coerce: &coerce,
        };
        let lint = |_: &str| -> Vec<Lint> { panic!("lint {}", 1) };
        let run = Run {
            settings: &settings,
            has_schema: parsed,
            lint: &lint,
            helpers: None,
            variants: None,
        };
        let source = parsed
            .then(|| Source::new(text, (3, 14)).map(Arc::new))
            .flatten();
        module(text, source.as_ref(), &run)
    }

    fn counted(text: &str) -> (usize, usize) {
        ANALYZED.with(|count| count.set(0));
        PARSED.with(|count| count.set(0));
        run_module(text, &[("param-count-mismatch", "error")], true);
        (
            ANALYZED.with(std::cell::Cell::get),
            PARSED.with(std::cell::Cell::get),
        )
    }

    #[test]
    fn a_driver_call_and_a_string_share_one_parse_and_one_analysis() {
        let text = concat!(
            "import psycopg\n",
            "conn = psycopg.connect('')\n",
            "cur = conn.cursor()\n",
            "cur.execute('--sql\\nSELECT $1::int AS x', (1,))\n",
            "q = '--sql\\nSELECT $1::int AS x'\n",
        );
        assert_eq!(counted(text), (1, 1));
    }

    #[test]
    fn a_driver_call_alone_is_neither_analyzed_nor_parsed_as_written() {
        let text = concat!(
            "import psycopg\n",
            "conn = psycopg.connect('')\n",
            "cur = conn.cursor()\n",
            "cur.execute('SELECT $1::int AS x', (1,))\n",
            "cur.execute('SELECT $1::int AS x', (2,))\n",
            "cur.execute('SELECT %s::int AS y', (3,))\n",
        );
        assert_eq!(counted(text), (0, 0));
    }

    fn walks(text: &str) -> usize {
        let options = Options::new("postgres", None, None, false).unwrap();
        let rules = RuleConfig::from_overrides(None).unwrap();
        let coerce = HashSet::new();
        let settings = Settings {
            options: &options,
            rules: &rules,
            index: None,
            module: None,
            coerce: &coerce,
        };
        let lint = |_: &str| -> Vec<Lint> { Vec::new() };
        let run = Run {
            settings: &settings,
            has_schema: false,
            lint: &lint,
            helpers: None,
            variants: None,
        };
        let source = Arc::new(Source::new(text, (3, 14)).unwrap());
        crate::scopes::WALKS.with(|count| count.set(0));
        diagnostics(&source, &run);
        crate::scopes::WALKS.with(std::cell::Cell::get)
    }

    #[test]
    fn a_module_is_walked_once_for_every_check() {
        let text = "import psycopg\nfrom typing import Annotated, Any\n\
                    from sqlty import SQL, SQL as S\n\
                    def q(sql: Annotated[str, SQL]): ...\n\
                    def run(cursor, x):\n\
                    \x20   q('--sql\\nSELECT 1')\n\
                    \x20   cursor.execute('--sql\\nSELECT %s', (x,))\n\
                    t = t'--sql\\nSELECT {1}'\n\
                    a = SQL[tuple[int], tuple[int]]('--sql\\nSELECT $1::int')\n\
                    b = SQL[Any]('--sql\\nSELECT 1')\n";
        assert_eq!(walks(text), 1);
    }

    #[test]
    fn a_module_the_prefilters_reject_is_not_walked() {
        assert_eq!(walks("x = 1\nprint(x)\n"), 0);
    }

    #[test]
    fn a_panic_becomes_an_internal_error() {
        let found = crash("q = 1\n", &[("sqruff", "warn")]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, Rule::InternalError);
        assert_eq!(found[0].severity, crate::rules::Severity::Error);
        assert_eq!(
            found[0].message,
            format!(
                "sqlty crashed checking this file: lint 1; please report it at {}",
                crate::report::issue_url("lint 1")
            )
        );
        let ignored = [("sqruff", "warn"), ("internal-error", "ignore")];
        assert!(crash("q = 1\n", &ignored).is_empty());
        assert!(crash("q = 1\n", &[]).is_empty());
    }

    #[test]
    fn a_panic_without_a_message_has_an_unknown_cause() {
        assert_eq!(crate::report::cause(&42), "unknown cause");
        assert_eq!(crate::report::cause(&"static"), "static");
    }

    #[test]
    fn a_dict_keeps_the_first_place_and_the_last_value_of_a_key() {
        assert_eq!(dict([("a", 1), ("b", 2), ("a", 3)]), [("a", 3), ("b", 2)]);
    }

    #[test]
    fn an_unknown_type_fits_and_a_bare_list_fits_only_a_list() {
        assert!(fits(None, Some("int")));
        assert!(fits(Some("list"), Some("list[int]")));
        assert!(!fits(Some("list"), Some("int")));
    }

    #[test]
    fn a_key_is_spelled_as_python_writes_it() {
        assert_eq!(key_text(&Key::Index(1)), "1");
        assert_eq!(key_text(&Key::Name("a".into())), "'a'");
    }

    const MIXED: &str = concat!(
        "import psycopg\n",
        "from sqlty import SQL\n",
        "conn = psycopg.connect('')\n",
        "cur = conn.cursor()\n",
        "cur.execute('--sql\\nSELECT $1::int AS x', (1, 2))\n",
        "cur.execute('--sql\\nSELECT $1::int AS x', ('a',))\n",
        "a = SQL[tuple[int]]('--sql\\nSELECT 1 AS x, 2 AS y')\n",
        "b = SQL[tuple[int, int]]('--sql\\nSELECT * FROM nowhere')\n",
        "c = '--sql\\nSELECT FROM'\n",
        "d = SQL[tuple[int]]('--sql\\nSELECT $1::int, nosuch(1)')\n",
    );

    /// The diagnostics of `text`, and the passes and lint calls it took.
    fn piped(
        text: &str,
        rules: &[(&str, &str)],
        passes: &[Pass],
        gated: bool,
    ) -> (Vec<(Rule, usize, String)>, Vec<Pass>, usize) {
        let options = Options::new("postgres", None, None, false).unwrap();
        let rules = RuleConfig::from_overrides(Some(
            rules
                .iter()
                .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
                .collect(),
        ))
        .unwrap();
        let coerce = HashSet::new();
        let settings = Settings {
            options: &options,
            rules: &rules,
            index: None,
            module: None,
            coerce: &coerce,
        };
        let linted = std::cell::Cell::new(0);
        let lint = |_: &str| -> Vec<Lint> {
            linted.set(linted.get() + 1);
            Vec::new()
        };
        let run = Run {
            settings: &settings,
            has_schema: false,
            lint: &lint,
            helpers: None,
            variants: None,
        };
        let source = Arc::new(Source::new(text, (3, 14)).unwrap());
        ENTERED.with(|entered| entered.borrow_mut().clear());
        let found = pipeline(&source, &run, passes, gated);
        let found = found
            .diagnostics
            .into_iter()
            .map(|reported| {
                let diagnostic = reported.diagnostic;
                (diagnostic.rule, diagnostic.line, diagnostic.message)
            })
            .collect();
        (
            found,
            ENTERED.with(|entered| entered.borrow().clone()),
            linted.get(),
        )
    }

    #[test]
    fn the_passes_run_in_the_order_of_the_table() {
        let (found, entered, linted) = piped(MIXED, &[("sqruff", "warn")], Pass::PIPELINE, true);
        assert_eq!(entered, Pass::PIPELINE);
        assert_eq!(linted, 1);
        assert!(!found.is_empty());
        let (_, entered, linted) = piped(MIXED, &[], Pass::PIPELINE, true);
        assert_eq!(linted, 0);
        let expected: Vec<Pass> = Pass::PIPELINE
            .iter()
            .copied()
            .filter(|pass| *pass != Pass::Sqruff)
            .collect();
        assert_eq!(entered, expected);
    }

    #[test]
    fn a_pass_without_a_pipeline_place_reports_nothing_itself() {
        let (found, entered, _) = piped(MIXED, &[], &Pass::ALL, true);
        assert_eq!(entered.len(), Pass::ALL.len() - 1);
        assert_eq!(found, piped(MIXED, &[], Pass::PIPELINE, true).0);
    }

    #[test]
    fn a_gated_pass_is_not_entered_when_its_rules_are_ignored() {
        let mut gated = 0;
        for pass in Pass::ALL.into_iter().filter(|pass| pass.gated()) {
            gated += 1;
            let mut rules: Vec<(&str, &str)> = pass
                .rules()
                .iter()
                .map(|rule| (rule.name(), "ignore"))
                .collect();
            rules.push(("sqruff", "warn"));
            if pass == Pass::Sqruff {
                rules.pop();
                rules.push(("sqruff", "ignore"));
            }
            let (found, entered, linted) = piped(MIXED, &rules, Pass::PIPELINE, true);
            assert!(!entered.contains(&pass), "{}", pass.name());
            assert_eq!(linted, usize::from(pass != Pass::Sqruff));
            let (all, entered, _) = piped(MIXED, &rules, Pass::PIPELINE, false);
            assert!(entered.contains(&pass), "{}", pass.name());
            if pass != Pass::Sqruff {
                assert_eq!(found, all, "{}", pass.name());
            }
        }
        assert_eq!(gated, 6);
    }

    #[test]
    fn the_ungated_pipeline_reports_what_the_gated_one_does() {
        for rules in [
            &[][..],
            &[
                ("param-type-mismatch", "ignore"),
                ("param-count-mismatch", "ignore"),
            ],
            &[("unparsed-query", "ignore"), ("unknown-function", "ignore")],
        ] {
            let gated = piped(MIXED, rules, Pass::PIPELINE, true).0;
            assert_eq!(gated, piped(MIXED, rules, Pass::PIPELINE, false).0);
        }
    }

    fn facts_of(text: &str) -> Source {
        Source::new(text, (3, 14)).unwrap()
    }

    const DRIVER: &str = concat!(
        "import psycopg\n",
        "conn = psycopg.connect('')\n",
        "cur = conn.cursor()\n",
        "cur.execute('SELECT %s::int AS x', (1,))\n",
    );

    #[test]
    fn driver_calls_are_the_calls_of_a_driver() {
        let source = facts_of(DRIVER);
        let facts = ModuleFacts::new(&source);
        let calls = driver_calls(DRIVER, &source, &facts);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].driver, Some("psycopg"));
        assert_eq!(calls[0].line, 4);
        assert!(calls[0].query_span.is_some());
        let plain = facts_of("x = 1\n");
        assert!(driver_calls("x = 1\n", &plain, &ModuleFacts::new(&plain)).is_empty());
    }

    #[test]
    fn template_calls_are_the_marked_t_strings() {
        let text = "t = t'--sql\\nSELECT {1}'\nu = t'SELECT {1}'\n";
        let source = facts_of(text);
        let facts = ModuleFacts::new(&source);
        let calls = template_calls(text, &source, &facts);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].driver, Some("psycopg"));
        assert_eq!(calls[0].line, 1);
        assert_eq!(calls[0].params.as_ref().map(Vec::len), Some(1));
        let plain = facts_of("x = 1\n");
        assert!(template_calls("x = 1\n", &plain, &ModuleFacts::new(&plain)).is_empty());
    }

    fn call(line: usize, span: Option<(usize, usize)>) -> Call<'static> {
        Call {
            line,
            query: Some("SELECT 1".to_string()),
            row: None,
            params: Some(Vec::new()),
            end_line: Some(line + 2),
            driver: None,
            single: None,
            dynamic: false,
            reason: None,
            variants: Vec::new(),
            declared: None,
            configured: true,
            query_span: span,
            literal: false,
        }
    }

    #[test]
    fn a_driver_call_keeps_the_parameters_of_a_helper_call_it_is() {
        let mut found = vec![call(1, Some((5, 9))), call(2, Some((6, 9))), call(3, None)];
        let drivers = [call(1, Some((5, 9)))];
        drop_params_checked_by_driver(&mut found, &drivers);
        let params: Vec<bool> = found.iter().map(|call| call.params.is_some()).collect();
        assert_eq!(params, [false, true, true]);
    }

    #[test]
    fn the_typed_lines_are_those_of_configured_calls() {
        let mut plain = call(10, None);
        plain.configured = false;
        let found = [call(3, None), plain];
        assert_eq!(typed_lines(&found), HashSet::from([3, 4, 5]));
    }

    fn variant(query: &str, lines: Vec<usize>) -> crate::variants::Variant<'static> {
        crate::variants::Variant {
            query: query.to_string(),
            declared: None,
            lines,
        }
    }

    fn reported(rule: Rule, line: usize, message: &str) -> Reported {
        Reported {
            diagnostic: Diagnostic {
                rule,
                message: message.to_string(),
                severity: crate::rules::Severity::Warn,
                line,
                span: 0..0,
                fix: Vec::new(),
            },
            redundant: None,
        }
    }

    #[test]
    fn a_branched_call_is_two_calls_and_one_report() {
        let mut branched = call(7, None);
        branched.variants = vec![variant("SELECT 1", vec![2]), variant("SELECT 2", vec![4])];
        let (branched, found) = expand_variants(vec![call(1, None), branched]);
        assert_eq!(branched.len(), 1);
        let lines: Vec<usize> = found.iter().map(|call| call.line).collect();
        assert_eq!(lines, [1, VARIANT_LINE, VARIANT_LINE + 1]);
        let queries: Vec<Option<&str>> = found.iter().map(|call| call.query.as_deref()).collect();
        assert_eq!(
            queries,
            [Some("SELECT 1"), Some("SELECT 1"), Some("SELECT 2")]
        );
        assert!(found.iter().all(|call| call.variants.is_empty()));
        let merged = merge_variants(
            vec![
                reported(Rule::UnknownTable, VARIANT_LINE, "same"),
                reported(Rule::UnknownTable, VARIANT_LINE + 1, "same"),
                reported(Rule::UnknownColumn, VARIANT_LINE + 1, "only second"),
                reported(Rule::SelectStar, 1, "elsewhere"),
            ],
            &branched,
        );
        let merged: Vec<(usize, &str)> = merged
            .iter()
            .map(|one| (one.diagnostic.line, one.diagnostic.message.as_str()))
            .collect();
        assert_eq!(
            merged,
            [
                (1, "elsewhere"),
                (7, "same"),
                (7, "only second (variant: line 4)"),
            ]
        );
    }

    #[test]
    fn a_stale_rewrite_on_a_typed_line_is_dropped() {
        let mut base: Vec<Diagnostic> = [
            reported(Rule::StaleRewrite, 3, "on a typed line"),
            reported(Rule::StaleRewrite, 9, "elsewhere"),
            reported(Rule::SelectStar, 3, "another rule"),
        ]
        .into_iter()
        .map(|one| one.diagnostic)
        .collect();
        drop_stale_rewrite_on_typed_lines(&mut base, &HashSet::from([3]));
        let kept: Vec<&str> = base.iter().map(|one| one.message.as_str()).collect();
        assert_eq!(kept, ["elsewhere", "another rule"]);
    }
}
