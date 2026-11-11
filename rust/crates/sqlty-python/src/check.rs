//! Every diagnostic of a module before inline suppression, as `sqlty._check.diagnostics`
//! reports them: the lexing pass of [`crate::checker`], then the calls that run a query
//! ([`crate::calls`]) against their parameters, names and row types ([`crate::rows`]).
//!
//! [`diagnostics`] answers `None` for a module that needs what is not read here yet.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, LazyLock};

use regex::Regex;
use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{Expr, ExprCall, Operator};
use ruff_text_size::Ranged;
use sqlty_sql::analyzer::{self, Nullability};

use crate::calls::{helper_calls, is_marked, Call};
use crate::checker::{self, Diagnostic};
use crate::index::HelperIndex;
use crate::rows::Reported;
use crate::rows::{
    alias_target, bind, row_queries, row_shape, value_diagnostics, Check, Found, RowQuery,
    RowShape, Settings,
};
use crate::rules::Rule;
use crate::scopes::{class_of, dotted, Key, Param};
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

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("a valid pattern")
}

/// `ParsedSource.may_need_rust_check`: whether the lexing pass can report anything.
fn may_need_lexing(text: &str, wrapper: &str) -> bool {
    // A marker split over literals: quotes, prefixes, whitespace and comments between its characters.
    static MARKER: LazyLock<Regex> = LazyLock::new(|| {
        let gap = r#"(?:[\s\x1c-\x1f"'rRuU]|#[^\n]*\n)*"#;
        pattern(&format!("(?i)-{gap}-{gap}s{gap}q{gap}l"))
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
    let Ok(start) = Regex::new(&format!(r"\b{}\[", regex::escape(wrapper))) else {
        return false;
    };
    for found in start.find_iter(text) {
        let mut depth = 0i32;
        for c in text[found.end()..].chars() {
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

struct DeclaredCalls<'a, 's> {
    source: &'a Source,
    wrapper: &'s str,
    found: Vec<Declared<'a>>,
}

impl<'a> Visitor<'a> for DeclaredCalls<'a, '_> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if let Some(found) = self.declared(call) {
                self.found.push(found);
            }
        }
        walk_expr(self, expr);
    }
}

impl<'a> DeclaredCalls<'a, '_> {
    fn declared(&self, call: &'a ExprCall) -> Option<Declared<'a>> {
        let Expr::Subscript(func) = call.func.as_ref() else {
            return None;
        };
        let Expr::Tuple(items) = func.slice.as_ref() else {
            return None;
        };
        let [_, params] = items.elts.as_slice() else {
            return None;
        };
        if dotted(&func.value).as_deref() != Some(self.wrapper) {
            return None;
        }
        let Some(Expr::StringLiteral(query)) = call.arguments.args.first() else {
            return None;
        };
        Some(Declared {
            line: self.source.line(call.start()),
            params,
            query: crate::parse::text(query, &self.source.text),
            span: (call.start().to_usize(), call.end().to_usize()),
        })
    }
}

/// `_declared_params`: each `SQL[R, P]("...")` call, in source order.
fn declared_params<'a>(source: &'a Source, wrapper: &str, import_line: &str) -> Vec<Declared<'a>> {
    if !may_wrap(&source.text, import_line) || !names_params(&source.text, wrapper) {
        return Vec::new();
    }
    let mut calls = DeclaredCalls {
        source,
        wrapper,
        found: Vec::new(),
    };
    calls.visit_body(source.body());
    calls.found.sort_by_key(|call| call.span);
    calls.found
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

/// `ParsedSource.literals`: the string literals that are not an operand of `+`.
struct Literals<'a> {
    source: &'a str,
    operands: HashSet<usize>,
    found: HashSet<String>,
}

impl<'a> Visitor<'a> for Literals<'a> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::BinOp(op) if op.op == Operator::Add => {
                self.operands.insert(op.left.start().to_usize());
                self.operands.insert(op.right.start().to_usize());
            }
            Expr::StringLiteral(text) if !self.operands.contains(&expr.start().to_usize()) => {
                self.found.insert(crate::parse::text(text, self.source));
            }
            Expr::FString(_) | Expr::TString(_) => {
                self.found
                    .extend(crate::unparse::constants(expr, self.source));
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

fn literals(source: &Source) -> HashSet<String> {
    let mut visitor = Literals {
        source: &source.text,
        operands: HashSet::new(),
        found: HashSet::new(),
    };
    visitor.visit_body(source.body());
    visitor.found
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

/// `sqlty._params.mismatch`: why a value of type `actual` can't be passed for a parameter
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

/// `sqlty._params.annotation_type`: the type a value annotated `expr` has, as argument
/// types are spelled.
fn annotation_type(expr: Option<&Expr>, imported: &HashMap<String, String>) -> Option<String> {
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
        analyzer::analyze_params(query, self.dialect(), self.run.settings.options.schema)
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
        is_marked(query)
            && self
                .literals
                .get_or_insert_with(|| literals(self.source))
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
        if !(self.enabled(Rule::ParamTypeMismatch)
            || self.enabled(Rule::ParamCountMismatch)
            || self.enabled(Rule::ExtraParam))
        {
            return;
        }
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
        let mut by_key: Vec<(Key, &analyzer::Param)> = Vec::new();
        for param in &params {
            let key = key_of(param);
            match by_key.iter_mut().find(|(known, _)| *known == key) {
                Some(known) => known.1 = param,
                None => by_key.push((key, param)),
            }
        }
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
        if !self.enabled(Rule::ParamsTypeMismatch) {
            return;
        }
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
        let mut pairs: Vec<(String, Option<&Expr>, &analyzer::Param, String)> = Vec::new();
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
                    Some(item),
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
            // Of the keys and the parameters, the last of a name stands, where its first is.
            let mut keys: Vec<&str> = Vec::new();
            for (key, _) in &info.fields {
                if !keys.contains(&key.as_str()) {
                    keys.push(key);
                }
            }
            let mut by_name: Vec<(&str, &analyzer::Param)> = Vec::new();
            for param in params {
                let Some(name) = param.name.as_deref() else {
                    continue;
                };
                match by_name.iter_mut().find(|(known, _)| *known == name) {
                    Some(known) => known.1 = param,
                    None => by_name.push((name, param)),
                }
            }
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
            for key in keys {
                let Some((_, param)) = by_name.iter().find(|(known, _)| *known == key) else {
                    continue;
                };
                pairs.push((
                    format!("{name}.{key}"),
                    info.annotations.get(key).map(|annotation| &annotation.expr),
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
            let Some(annotation) = annotation else {
                continue;
            };
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
            if let Some((rule, message)) =
                checker::select_star_finding(query, self.dialect(), self.run.has_schema)
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
        static FORMAT_FIELD: LazyLock<Regex> = LazyLock::new(|| pattern(r"\{\w*\}"));
        if !self.enabled(Rule::UnparsedQuery) {
            return;
        }
        for (query, line, span) in self.unread() {
            let Some(error) = analyzer::parse_error(&query, self.dialect()) else {
                continue;
            };
            let mut message = format!("query does not parse: {error}");
            if error.contains("found: {") {
                if let Some(field) = FORMAT_FIELD.find(&query) {
                    message.push_str(&format!(
                        "; `{}` is a `.format()` field: fill it with `sql.SQL(...).format(...)`, or pass a value as `%s`",
                        field.as_str()
                    ));
                }
            }
            self.ctx.report(Rule::UnparsedQuery, message, line, span);
        }
    }

    fn names(&mut self) {
        use sqlty_sql::diagnostics::NameRule;
        if !NameRule::ALL
            .iter()
            .any(|rule| self.enabled(Rule::Name(*rule)))
        {
            return;
        }
        for (query, line, span) in self.unread() {
            let schema = self.run.settings.options.schema;
            let (_, found) = analyzer::analyze_with_diagnostics(&query, self.dialect(), schema);
            for diagnostic in found {
                self.ctx
                    .report(Rule::Name(diagnostic.rule), diagnostic.message, line, span);
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
        if let Expr::Subscript(sub) = declared {
            if matches!(
                dotted(&sub.value).as_deref(),
                Some("tuple" | "typing.Tuple")
            ) {
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
    let calls = helper_calls(source, helpers, module, variants);
    let mut read: Option<HashSet<String>> = None;
    let mut by_marker = |query: &str| {
        is_marked(query) && read.get_or_insert_with(|| literals(source)).contains(query)
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

/// `sqlty._check.diagnostics`: every diagnostic of `source` before inline suppression, by
/// line.
pub fn diagnostics(source: &Arc<Source>, run: &Run<'_>) -> Found {
    static MARKED_TEMPLATE: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r#"(?:^|[^\w])(?:[tT][rR]?|[rR][tT])(?:'|")+[\s\x1c-\x1f]*--[\s\x1c-\x1f]?sql"#)
    });
    let settings = run.settings;
    let (options, rules) = (settings.options, settings.rules);
    let text = &*source.text;
    let wrapper = options.style.wrapper_name.as_str();
    let import_line = options.style.import_line.as_str();

    let mut base = Vec::new();
    if may_need_lexing(text, wrapper) {
        let any_calls: Vec<usize> = if text.contains("Any") && may_wrap(text, import_line) {
            module::any_calls(source.body(), &source.typing)
                .into_iter()
                .filter(|(_, name)| {
                    name == wrapper
                        || name
                            .strip_suffix(wrapper)
                            .is_some_and(|prefix| prefix.ends_with('.'))
                })
                .map(|(offset, _)| offset)
                .collect()
        } else {
            Vec::new()
        };
        base = checker::check_source(text, options, rules, &any_calls);
        checker::add_fix_imports(text, options, &mut base);
    }

    let mut found = helper_calls(source, run.helpers, run.settings.module, run.variants);
    let mut driver_calls: Vec<Call<'_>> = Vec::new();
    if drivers::may_hold(text) {
        for call in drivers::find(text, source.body(), &source.imports) {
            driver_calls.push(Call {
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
            });
        }
    }
    if MARKED_TEMPLATE.is_match(text) {
        for template in templates::of(text, source.body(), &source.imports) {
            let Some(query) = template.query.filter(|query| is_marked(query)) else {
                continue;
            };
            let params = template.params.into_iter().enumerate();
            driver_calls.push(Call {
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
    }
    // A driver call also configured as a helper has its parameters checked once, as the driver's.
    let by_driver: HashSet<(usize, usize)> = driver_calls
        .iter()
        .filter_map(|call| call.query_span)
        .collect();
    for call in &mut found {
        if call
            .query_span
            .is_some_and(|span| by_driver.contains(&span))
        {
            call.params = None;
        }
    }
    let typed: HashSet<usize> = found.iter().flat_map(Call::typed_lines).collect();
    // Each variant of a call built in branches is checked as a call of its own.
    let (branched, mut found): (Vec<Call<'_>>, Vec<Call<'_>>) =
        found.into_iter().partition(|call| call.variants.len() > 1);
    let variants = branched.iter().flat_map(|call| {
        call.variants.iter().map(move |variant| Call {
            query: Some(variant.query.clone()),
            declared: variant.declared.clone(),
            variants: Vec::new(),
            ..call.clone()
        })
    });
    let variants: Vec<Call<'_>> = variants
        .enumerate()
        .map(|(index, call)| Call {
            line: VARIANT_LINE + index,
            ..call
        })
        .collect();
    found.extend(variants);
    base.retain(|diagnostic| {
        !(diagnostic.rule == Rule::StaleRewrite && typed.contains(&diagnostic.line))
    });
    if rules.is_enabled(Rule::Sqruff) {
        let severity = rules.severity(Rule::Sqruff);
        base.extend((run.lint)(text).into_iter().map(|lint| Diagnostic {
            rule: Rule::Sqruff,
            message: lint.message,
            severity,
            line: source.lines.line(lint.query_start),
            span: lint.span,
            fix: Vec::new(),
        }));
    }
    let queries: Vec<RowQuery> = if may_wrap(text, import_line) {
        row_queries(source, settings)
    } else {
        Vec::new()
    };
    let declared = declared_params(source, wrapper, import_line);
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
    if queries.is_empty() && found.is_empty() && driver_calls.is_empty() && declared.is_empty() {
        return out;
    }

    let dialect = options.dialect.name();
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
    let mut module = Module {
        run,
        ctx: Check::new(source, settings, queries, named, &names),
        source,
        found,
        driver_calls,
        declared,
        rows,
        literals: None,
    };
    module.unchecked_queries();
    module.check_params();
    module.declared_params();
    module.ctx.typeddict_inheritance();
    module.select_star();
    module.unparsed();
    module.names();
    let bound = module.bound_params();
    module.ctx.structural(&bound);
    module.ctx.aliases();
    module.values();
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
