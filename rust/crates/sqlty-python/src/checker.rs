//! Lint checker for SQL type issues in Python source code.
//!
//! Checks for:
//! - `stale-rewrite`: `--sql` strings not wrapped in `SQL[T](...)`
//! - `stale-type`: `SQL[T]` annotation doesn't match current inference
//! - `select-star`: `SELECT *` usage
//! - `select-star-no-schema`: `SELECT *` with no schema available
//! - `inline-sql-comment`: `"--sql SELECT ..."` (space) makes query a SQL comment
//! - `untyped-query`: `SQL[UNTYPED]` query has no inferable row type
//! - `fstring-skipped` / `tstring-skipped`: interpolated `--sql` strings are never rewritten
//! - `unchecked-query`: a `--sql` string that is not plain literals (raw, bytes, `SQL[T](q + x)`),
//!   unless the wrapper call is one of `any_calls`

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::edit::Edit;
use crate::options::{Dialect, Options};
use crate::rewriter::infer_sql_content as infer_from_sql_content;
use crate::rewriter::{
    after_marker, decode_escapes, ends_marker_line, extract_string_content, marked_query,
    strip_line_break, wraps, ImportStyle, Spelling,
};
use crate::rules::{Rule, RuleConfig, Severity};
use crate::scan::Scan;
use crate::tokens::{content, Call};
use sqlty_sql::analyzer::{
    analyze_with_diagnostics, expand_star, parse_error, result_stars, row_type, SchemaData,
};
use sqlty_sql::diagnostics::NameRule;
use std::ops::Range;

/// `--sql SELECT ...`: a space after the marker, then the start of a SQL statement,
/// which makes the whole query a SQL comment. Each keyword needs what its statement
/// needs (`DELETE FROM`, `UPDATE t SET`, ...), so prose such as `"--sql delete me
/// later"` does not match.
const INLINE_SQL_COMMENT: &str = concat!(
    r"--sql[ \t]+(?i:",
    r"SELECT\s+[^\s;]",
    r"|WITH\s+(?:RECURSIVE\s+)?\w+[^;]*?\bAS\s*\(",
    r"|INSERT\s+INTO\b",
    r#"|UPDATE\s+[\w."]+(?:\s+(?:AS\s+)?\w+)?\s+SET\b"#,
    r"|DELETE\s+FROM\b",
    r"|VALUES\s*\(",
    r"|(?:CREATE|ALTER|DROP)\s+(?:(?:OR\s+REPLACE|TEMP|TEMPORARY|UNIQUE|MATERIALIZED)\s+)*",
    r"(?:TABLE|VIEW|INDEX|SCHEMA|SEQUENCE|FUNCTION|TYPE|EXTENSION|DATABASE|TRIGGER|PROCEDURE|ROLE)\b",
    r"|TRUNCATE\s+(?:TABLE\s+)?\w",
    r"|MERGE\s+INTO\b",
    r"|EXPLAIN\s+(?:(?:ANALYZE|VERBOSE)\s+)*(?:SELECT|WITH|INSERT|UPDATE|DELETE|VALUES)\b",
    r")"
);

/// String content starting with `INLINE_SQL_COMMENT`.
pub(crate) static INLINE_SQL_COMMENT_CONTENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("^{INLINE_SQL_COMMENT}")).unwrap());

/// Plain strings opening with a `--sql` marker line, which MySQL and MariaDB reject:
/// they read `--` as a comment only before whitespace. Each with its line and the edit
/// to `-- sql`.
fn mysql_markers(scan: &Scan) -> Vec<(usize, Edit)> {
    let source = scan.source();
    scan.plain_literals()
        .filter_map(|(start, end)| {
            let literal = &source[start..end];
            let marked = extract_string_content(literal).trim_start();
            marked
                .strip_prefix("--sql")
                .is_some_and(ends_marker_line)
                .then(|| {
                    let at = start + literal.find(marked).unwrap_or_default();
                    let edit = Edit::new(at..at + "--sql".len(), "-- sql");
                    (scan.lines.line_at(start), edit)
                })
        })
        .collect()
}

/// `"--sql SELECT ..."` to `"--sql\nSELECT ..."`: the escape replaces the space after the
/// marker of the string literal at `span`.
fn inline_comment_edit(source: &str, span: &Range<usize>) -> Edit {
    let literal = &source[span.clone()];
    let quoted = literal.trim_start_matches(['u', 'U']);
    let quote = QUOTES
        .into_iter()
        .find(|q| quoted.starts_with(q))
        .unwrap_or("\"");
    let at = span.end - quoted.len() + quote.len() + "--sql".len();
    Edit::new(at..at + 1, "\\n")
}

pub(crate) const QUOTES: [&str; 4] = ["\"\"\"", "'''", "\"", "'"];

/// A diagnostic result from checking source code.
pub struct Diagnostic {
    /// The rule that fired.
    pub rule: Rule,
    /// What is wrong, for the user.
    pub message: String,
    /// How loudly the rule reports, as configured.
    pub severity: Severity,
    /// 1-based line of the source the diagnostic points at.
    pub line: usize,
    /// The edits `sqlty check --fix` makes for it; empty when it has no fix.
    pub fix: Vec<Edit>,
}

/// Check Python source code for SQL type issues.
pub fn check_source(
    source: &str,
    options: &Options,
    rules: &RuleConfig,
    any_calls: &[usize],
) -> Vec<Diagnostic> {
    let scan = Scan::new(source, &options.style.wrapper_name);
    let mut checker = Checker {
        scan: &scan,
        options,
        rules,
        any_calls: sorted(any_calls),
        diagnostics: Vec::new(),
        spelling: Spelling::of(&scan),
        inferred: HashMap::new(),
    };
    if options.dialect == Dialect::MySql {
        checker.mysql_markers();
    }
    if rules.is_enabled(Rule::StaleRewrite) {
        checker.stale_rewrites();
    }
    checker.unwrapped_strings();
    checker.wrapped_calls();
    checker.finish()
}

/// What the steps of [`check_source`] share: the scanned source, the settings and the
/// diagnostics found so far.
struct Checker<'a> {
    scan: &'a Scan<'a>,
    options: &'a Options<'a>,
    rules: &'a RuleConfig,
    /// Offsets of the calls whose `T` is `typing.Any`, ascending.
    any_calls: Vec<usize>,
    diagnostics: Vec<Diagnostic>,
    /// How the module can spell the row types it names.
    spelling: Spelling,
    /// Row types the name diagnostics' analysis already inferred, by marked query, so the
    /// annotation checks don't analyze a query twice.
    inferred: HashMap<String, Option<String>>,
}

impl Checker<'_> {
    fn report(&mut self, rule: Rule, message: String, line: usize) {
        self.report_fix(rule, message, line, Vec::new());
    }

    fn report_fix(&mut self, rule: Rule, message: String, line: usize, fix: Vec<Edit>) {
        if self.rules.is_enabled(rule) {
            self.diagnostics.push(Diagnostic {
                rule,
                message,
                severity: self.rules.severity(rule),
                line,
                fix,
            });
        }
    }

    /// Plain strings whose marker line MySQL reads as an expression.
    fn mysql_markers(&mut self) {
        for (line, edit) in mysql_markers(self.scan) {
            self.report_fix(
                Rule::MysqlSqlMarker,
                r#"MySQL reads "--sql" as an expression, not a comment; use "-- sql""#.to_string(),
                line,
                vec![edit],
            );
        }
    }

    /// `--sql` strings that `rewrite` would wrap.
    fn stale_rewrites(&mut self) {
        let scan = self.scan;
        for edit in wraps(scan, self.options, &|_| false) {
            self.report_fix(
                Rule::StaleRewrite,
                "Source has --sql strings that aren't wrapped in SQL[T]".to_string(),
                scan.lines.line_at(edit.range.start),
                vec![edit],
            );
        }
    }

    /// Strings outside any wrapper call: the ones `rewrite` skips, and `--sql SELECT`.
    fn unwrapped_strings(&mut self) {
        let scan = self.scan;
        for group in scan.groups() {
            let enclosing = scan.enclosing_call(group);
            if let Some(rule) = group.skipped {
                if !(rule == Rule::UncheckedQuery
                    && enclosing.is_some_and(|call| call.is_any(&self.any_calls)))
                {
                    let message = match scan.template_blocker(&group.tokens) {
                        Some(field) if rule == Rule::TstringSkipped => {
                            format!("t-string with --sql prefix skipped (the field {field} changes the SQL)")
                        }
                        _ => skipped_message(rule),
                    };
                    self.report(rule, message, group.line);
                }
                continue;
            }
            if enclosing.is_some() {
                continue;
            }
            let first = scan.tokens[group.tokens[0]].span.clone();
            let content = scan.content(group);
            // psycopg's `sql.SQL("... {} ...")` is a template its `.format()` fills; the
            // helper call that receives it checks the composed query.
            let template = content.contains('{') && composes(scan.source(), first.start);
            self.marker_and_syntax(&content, group.line, &first, template);
            if after_marker(&content).is_some() && !template {
                self.name_diagnostics(&content, group.line);
            }
        }
    }

    fn wrapped_calls(&mut self) {
        let scan = self.scan;
        for call in &scan.calls {
            self.wrapped_call(call);
        }
    }

    fn wrapped_call(&mut self, call: &Call) {
        let scan = self.scan;
        let line = scan.lines.line_at(call.span.start);
        let Some(sql_content) = call.content(scan.source()) else {
            self.unchecked_argument(call, line);
            return;
        };
        if after_marker(&sql_content).is_none() {
            return;
        }
        self.marker_and_syntax(&sql_content, line, &call.strings[0], false);
        self.name_diagnostics(&sql_content, line);
        self.select_star(call, &sql_content, line);
        self.annotation(call, &sql_content, line);
    }

    /// `inline-sql-comment` and `unparsed-query` of marked string content, which starts in
    /// the literal at `first`; a `template` is not parsed.
    fn marker_and_syntax(
        &mut self,
        sql_content: &str,
        line: usize,
        first: &Range<usize>,
        template: bool,
    ) {
        if INLINE_SQL_COMMENT_CONTENT.is_match(sql_content) {
            let source = self.scan.source();
            let literal = &source[first.clone()];
            let plain = literal
                .trim_start_matches(['u', 'U'])
                .starts_with(['"', '\'']);
            let fix = (plain
                && INLINE_SQL_COMMENT_CONTENT.is_match(extract_string_content(literal)))
            .then(|| inline_comment_edit(source, first));
            self.report_fix(
                Rule::InlineSqlComment,
                INLINE_MESSAGE.to_string(),
                line,
                fix.into_iter().collect(),
            );
        }
        if self.rules.is_enabled(Rule::UnparsedQuery) && !template {
            if let Some(error) = marked_query(sql_content)
                .and_then(|sql| parse_error(&sql, self.options.dialect.name()))
            {
                self.report(Rule::UnparsedQuery, unparsed_message(&error), line);
            }
        }
    }

    /// `unchecked-query` for a call whose argument is not plain literals, unless it is
    /// interpolated (reported as a skipped f- or t-string) or opts out with `Any`.
    fn unchecked_argument(&mut self, call: &Call, line: usize) {
        let source = self.scan.source();
        let interpolated = call.strings.iter().any(|span| {
            source[span.clone()]
                .split(['"', '\''])
                .next()
                .is_some_and(|prefix| prefix.contains(['f', 'F', 't', 'T']))
        });
        if !interpolated
            && !call.is_any(&self.any_calls)
            && after_marker(&content(source, &call.strings)).is_some()
        {
            self.report(
                Rule::UncheckedQuery,
                skipped_message(Rule::UncheckedQuery),
                line,
            );
        }
    }

    /// Why a query has untyped columns (the `name` rules).
    fn name_diagnostics(&mut self, sql_content: &str, line: usize) {
        let enabled = NameRule::ALL
            .iter()
            .any(|&rule| self.rules.is_enabled(Rule::Name(rule)));
        if !enabled {
            return;
        }
        if let Some(query) = marked_query(sql_content) {
            let (columns, found) =
                analyze_with_diagnostics(&query, self.options.dialect.name(), self.options.schema);
            let inferred = columns
                .ok()
                .flatten()
                .and_then(|columns| row_type(&columns));
            self.inferred.insert(query, inferred);
            for found in found {
                self.report(Rule::Name(found.rule), found.message, line);
            }
        }
    }

    /// `select-star` and `select-star-no-schema`: a `*` or `t.*` that shapes the result.
    /// With a schema, a query in one literal is fixed by expanding the star.
    fn select_star(&mut self, call: &Call, sql_content: &str, line: usize) {
        let Some(query) = marked_query(sql_content) else {
            return;
        };
        let Some((rule, message)) = select_star_finding(
            &query,
            self.options.dialect.name(),
            self.options.schema.is_some(),
        ) else {
            return;
        };
        let fix = match (rule, call.strings.as_slice(), self.options.schema) {
            (Rule::SelectStar, [span], Some(schema)) => expanded_literal(
                self.scan.source(),
                span,
                sql_content,
                self.options.dialect.name(),
                schema,
            ),
            _ => None,
        };
        self.report_fix(rule, message, line, fix.into_iter().collect());
    }

    /// The row type of marked string content, from the name diagnostics' analysis when
    /// it ran.
    fn infer(&self, sql_content: &str) -> Option<String> {
        if !self.inferred.is_empty() {
            if let Some(inferred) = marked_query(sql_content).and_then(|q| self.inferred.get(&q)) {
                return inferred.clone();
            }
        }
        infer_from_sql_content(
            sql_content,
            self.options.dialect.name(),
            self.options.schema,
        )
    }

    /// Why a query has no row type, and what to do about it: the first name or construct
    /// the analyzer could not type, else the untyped columns.
    fn untyped_cause(&self, sql_content: &str) -> String {
        let has_schema = self.options.schema.is_some_and(|schema| !schema.is_empty());
        let Some(query) = marked_query(sql_content) else {
            return "add casts (for example id::INTEGER) or a schema".to_string();
        };
        let (columns, diagnostics) =
            analyze_with_diagnostics(&query, self.options.dialect.name(), self.options.schema);
        if let Some(diagnostic) = diagnostics.first() {
            let hint = match diagnostic.rule {
                NameRule::UnknownFunction => {
                    "; cast its result, or declare the function in the schema"
                }
                NameRule::UnknownType => "; declare the type in the schema",
                NameRule::UnsupportedSyntax => "; cast it to a type",
                _ => "",
            };
            return format!("{}{hint}", diagnostic.message);
        }
        let untyped: Vec<String> = columns
            .ok()
            .flatten()
            .unwrap_or_default()
            .iter()
            .filter(|column| column.python_type.is_none())
            .map(|column| format!("`{}`", column.name))
            .collect();
        let advice = if has_schema {
            "cast it to a type"
        } else {
            "add casts (for example id::INTEGER) or a schema"
        };
        match untyped.as_slice() {
            [] => advice.to_string(),
            [star] if star == "`*`" => "`*` is not expanded; add a schema".to_string(),
            [column] => format!("column {column} is untyped; {advice}"),
            columns => format!("columns {} are untyped; {advice}", columns.join(", ")),
        }
    }

    /// `untyped-query`, `stale-type` and the literal rules, against the inferred row type.
    fn annotation(&mut self, call: &Call, sql_content: &str, line: usize) {
        let annotated = &self.scan.source()[call.annotation.clone()];
        let wrapper = if call.template {
            self.options.style.template_name()
        } else {
            self.options.style.wrapper_name.clone()
        };
        let untyped_name = self.options.style.untyped_name();
        if annotated == untyped_name {
            let untyped = format!("{wrapper}[{untyped_name}]");
            // A `*` the schema cannot resolve infers no type, so none is guessed.
            let current = self
                .infer(sql_content)
                .map(|current| self.spelling.respell(current));
            let message = match &current {
                Some(current) => {
                    format!("{untyped} can now be {wrapper}[{current}]; run sqlty check --fix")
                }
                None => format!(
                    "{untyped} has no inferable row type: {}",
                    self.untyped_cause(sql_content)
                ),
            };
            let fix = current.map(|current| Edit::new(call.annotation.clone(), current));
            self.report_fix(Rule::UntypedQuery, message, line, fix.into_iter().collect());
            return;
        }

        if is_scalar(annotated) && self.rules.is_enabled(Rule::NarrowType) {
            if let Some(current) = self
                .infer(sql_content)
                .map(|current| self.spelling.respell(current))
                .filter(|current| current.starts_with("tuple["))
            {
                let wrapper = if call.template { "SQLTemplate" } else { "SQL" };
                self.report_fix(
                    Rule::NarrowType,
                    format!("{wrapper}[{annotated}] should be {wrapper}[{current}]: a row is never a single value"),
                    line,
                    vec![Edit::new(call.annotation.clone(), current)],
                );
            }
            return;
        }
        let literals = self.rules.is_enabled(Rule::LiteralMismatch)
            || self.rules.is_enabled(Rule::RedundantLiteral);
        if !(annotated.starts_with("tuple[")
            && (self.rules.is_enabled(Rule::StaleType)
                || self.rules.is_enabled(Rule::NarrowType)
                || literals))
        {
            return;
        }
        if let Some(current) = self
            .infer(sql_content)
            .map(|current| self.spelling.respell(current))
        {
            let spelled = spell_unions(annotated);
            if !refines_tuple(&spelled, &current) {
                self.report_fix(
                    if admits_tuple(&spelled, &current) {
                        Rule::StaleType
                    } else {
                        Rule::NarrowType
                    },
                    format!(
                        "{}[{annotated}] should be {}[{current}]",
                        if call.template { "SQLTemplate" } else { "SQL" },
                        if call.template { "SQLTemplate" } else { "SQL" }
                    ),
                    line,
                    vec![Edit::new(call.annotation.clone(), current.clone())],
                );
            }
            for (rule, message) in literal_findings(&spelled, &current) {
                self.report(rule, message, line);
            }
        }
    }

    fn finish(mut self) -> Vec<Diagnostic> {
        self.diagnostics.sort_by_key(|d| d.line);
        // One report per line, rule and message, with the fixes of all of them.
        self.diagnostics.dedup_by(|later, kept| {
            let same =
                later.line == kept.line && later.rule == kept.rule && later.message == kept.message;
            if same {
                kept.fix.append(&mut later.fix);
            }
            same
        });
        self.diagnostics
    }
}

/// Whether the literal at `at` is the first argument of a psycopg `sql.SQL(...)` call.
fn composes(source: &str, at: usize) -> bool {
    source[..at]
        .trim_end()
        .strip_suffix('(')
        .is_some_and(|callee| callee.trim_end().ends_with(".SQL"))
}

/// The string literal at `span`, holding all of `content`, with its `*` expanded; `None`
/// when the expansion cannot be written back in the same quotes.
fn expanded_literal(
    source: &str,
    span: &Range<usize>,
    content: &str,
    dialect: &str,
    schema: &SchemaData,
) -> Option<Edit> {
    let literal = &source[span.clone()];
    let quoted = literal.trim_start_matches(['u', 'U']);
    let prefix = &literal[..literal.len() - quoted.len()];
    let quote = QUOTES
        .into_iter()
        .find(|q| quoted.starts_with(q))
        .unwrap_or("\"");
    let caps = SQL_PREFIX.captures(content)?;
    let (marker, query) = (&caps[1], &caps[2]);
    let quote_char = &quote[..1];
    if !query.contains('*') || query.contains('\\') || query.contains(quote_char) {
        return None;
    }
    let expanded = expand_star(query, dialect, Some(schema))?;
    if expanded.contains('\\')
        || expanded.contains(quote_char)
        || (quote.len() == 1 && expanded.contains('\n'))
    {
        return None;
    }
    Some(Edit::new(
        span.clone(),
        format!("{prefix}{quote}{marker}{expanded}{quote}"),
    ))
}

/// A `--sql` marker line at the start of string content.
static SQL_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^(\s*(?:--sql|-- sql)(?:\\r\\n|\\n|\r?\n))(.*)").unwrap());

const INLINE_MESSAGE: &str = r#""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty check --fix"#;

fn skipped_message(rule: Rule) -> String {
    match rule {
        Rule::TstringSkipped => {
            "t-string with --sql prefix skipped (raw t-strings are not read as queries)"
        }
        Rule::FstringSkipped => {
            "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
        }
        _ => "--sql query is not plain string literals, so it is not checked or rewritten",
    }
    .to_string()
}

/// Message of `unparsed-query`, given sqlparser's error (see `analyzer::parse_error`).
pub(crate) fn unparsed_message(error: &str) -> String {
    format!("query does not parse: {error}")
}

/// What `sqlty check --statistics` counts a string as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    /// A query; the text is the marked query.
    Query,
    /// A string the rule skips; the text is empty.
    Skipped(Rule),
}

impl SiteKind {
    /// `query`, or the name of the rule that skips the string.
    pub fn name(self) -> &'static str {
        match self {
            SiteKind::Query => "query",
            SiteKind::Skipped(rule) => rule.name(),
        }
    }
}

/// `select-star` (with a schema) or `select-star-no-schema` for a query with a `*` or
/// `t.*` that shapes its result, naming the clauses; `None` without one.
///
/// Example:
/// ```
/// use sqlty_python::checker::select_star_finding;
/// use sqlty_python::rules::Rule;
///
/// let (rule, message) = select_star_finding("SELECT u.* FROM users u", "postgres", true).unwrap();
/// assert_eq!(rule, Rule::SelectStar);
/// assert_eq!(message, "SELECT u.* used — expand to explicit column list");
/// assert!(select_star_finding("SELECT count(*) FROM users", "postgres", true).is_none());
/// ```
pub fn select_star_finding(query: &str, dialect: &str, has_schema: bool) -> Option<(Rule, String)> {
    let stars = result_stars(query, dialect).unwrap_or_else(|| {
        // A query that doesn't parse falls back to the text.
        let upper = query.to_uppercase();
        if upper.contains("SELECT *") || upper.contains("SELECT\n*") {
            vec!["SELECT *".to_string()]
        } else {
            vec![]
        }
    });
    if stars.is_empty() {
        return None;
    }
    let stars = stars.join(" and ");
    Some(if has_schema {
        (
            Rule::SelectStar,
            format!("{stars} used — expand to explicit column list"),
        )
    } else {
        (
            Rule::SelectStarNoSchema,
            format!("{stars} used but no schema available"),
        )
    })
}

/// Each query `sqlty check` sees, as `(line, kind, text)` for `--statistics`.
pub fn query_sites(
    source: &str,
    style: &ImportStyle,
    any_calls: &[usize],
) -> Vec<(usize, SiteKind, String)> {
    let any_calls = sorted(any_calls);
    let scan = Scan::new(source, &style.wrapper_name);
    let mut sites = Vec::new();
    let mut skipped: Vec<usize> = Vec::new();
    for group in scan.groups() {
        let enclosing = scan.enclosing_call(group);
        if let Some(rule) = group.skipped {
            skipped.push(group.start);
            if !(rule == Rule::UncheckedQuery
                && enclosing.is_some_and(|call| call.is_any(&any_calls)))
            {
                sites.push((group.line, SiteKind::Skipped(rule), String::new()));
            }
            continue;
        }
        if enclosing.is_some() {
            continue;
        }
        if let Some(sql) = marked_query(&scan.content(group)) {
            sites.push((group.line, SiteKind::Query, sql));
        }
    }
    for call in &scan.calls {
        let line = scan.lines.line_at(call.span.start);
        match call.content(source) {
            Some(text) => {
                if let Some(sql) = marked_query(&text) {
                    sites.push((line, SiteKind::Query, sql));
                }
            }
            None if after_marker(&content(source, &call.strings)).is_some()
                && !call.is_any(&any_calls)
                && !holds_start(&skipped, &call.span) =>
            {
                sites.push((line, SiteKind::Skipped(Rule::UncheckedQuery), String::new()));
            }
            None => {}
        }
    }
    sites.sort_by_key(|site| site.0);
    sites
}

/// `offsets`, ascending, for [`Call::is_any`].
fn sorted(offsets: &[usize]) -> Vec<usize> {
    let mut offsets = offsets.to_vec();
    offsets.sort_unstable();
    offsets
}

/// Whether one of `starts`, ascending, lies in `span`.
fn holds_start(starts: &[usize], span: &Range<usize>) -> bool {
    let first = starts.partition_point(|&start| start < span.start);
    starts.get(first).is_some_and(|&start| start < span.end)
}

/// `(line, annotation, query)` for each `{wrapper}[T](...)` call; `query` is the string
/// without its marker, `None` unless the call holds a marked string literal.
pub fn wrapped_queries(source: &str, style: &ImportStyle) -> Vec<(usize, String, Option<String>)> {
    let scan = Scan::new(source, &style.wrapper_name);
    scan.calls
        .iter()
        .map(|call| {
            let query = call.content(source).and_then(|content| {
                let decoded = decode_escapes(&content);
                let rest = after_marker(&decoded)?;
                Some(strip_line_break(rest).unwrap_or(rest).trim().to_string())
            });
            (
                scan.lines.line_at(call.span.start),
                source[call.annotation.clone()].to_string(),
                query,
            )
        })
        .collect()
}

/// Whether `annotated` is a builtin scalar or collection, such as `int` or `list[str]`:
/// a driver returns each row as a tuple, a dict or an instance of a row class.
fn is_scalar(annotated: &str) -> bool {
    let members = split_top(annotated, '|');
    members.iter().any(|member| member.trim() != "None")
        && members.iter().all(|member| {
            let base = member.trim().split('[').next().unwrap_or_default();
            matches!(
                base.strip_prefix("typing.").unwrap_or(base),
                "None"
                    | "int"
                    | "str"
                    | "float"
                    | "bool"
                    | "bytes"
                    | "complex"
                    | "list"
                    | "set"
                    | "frozenset"
                    | "List"
                    | "Set"
                    | "FrozenSet"
            )
        })
}

/// The items of a `tuple[...]` type, split at its top-level commas.
fn tuple_items(ty: &str) -> Option<Vec<&str>> {
    let inner = ty.strip_prefix("tuple[")?.strip_suffix(']')?;
    let (mut items, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, byte) in inner.bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b',' if depth == 0 => {
                items.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(inner[start..].trim());
    Some(items)
}

/// A member of a `Literal`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Lit {
    Str(String),
    Int(i64),
}

impl Lit {
    fn show(&self) -> String {
        match self {
            Lit::Str(text) => format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'")),
            Lit::Int(number) => number.to_string(),
        }
    }
}

/// `text` split at `sep` outside brackets and quotes.
fn split_top(text: &str, sep: char) -> Vec<&str> {
    let (mut parts, mut depth, mut quote, mut start) = (Vec::new(), 0i32, None, 0);
    let mut escaped = false;
    for (at, ch) in text.char_indices() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '[' => depth += 1,
            ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

/// The members of a tuple item, with `Optional[X]`, `Union[A, B]` and a quoted annotation
/// spelled out: `Optional[str]` and `"str | None"` are `["str", "None"]`.
fn union_members(item: &str) -> Vec<&str> {
    let parts = split_top(item, '|');
    if parts.len() > 1 {
        return parts.into_iter().flat_map(union_members).collect();
    }
    let item = item.trim();
    let quoted = ['"', '\''].iter().find_map(|&quote| {
        item.strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
    });
    if let Some(inner) = quoted {
        return union_members(inner);
    }
    let bare = item.strip_prefix("typing.").unwrap_or(item);
    if let Some(inner) = bare
        .strip_prefix("Optional[")
        .and_then(|rest| rest.strip_suffix(']'))
    {
        let mut members = union_members(inner);
        members.push("None");
        return members;
    }
    if let Some(inner) = bare
        .strip_prefix("Union[")
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return split_top(inner, ',')
            .into_iter()
            .flat_map(union_members)
            .collect();
    }
    vec![item]
}

/// `annotated` with each `Optional[X]`, `Union[A, B]` or quoted item spelled `A | B | None`,
/// as sqlty writes it.
fn spell_unions(annotated: &str) -> String {
    let Some(items) = tuple_items(annotated) else {
        return annotated.to_string();
    };
    let items: Vec<String> = items
        .iter()
        .map(|item| {
            let members = union_members(item);
            let mut spelled: Vec<&str> = members.iter().copied().filter(|m| *m != "None").collect();
            if spelled.len() < members.len() {
                spelled.push("None");
            }
            spelled.join(" | ")
        })
        .collect();
    format!("tuple[{}]", items.join(", "))
}

/// Whether two tuple items are the same union, in any order and spelling.
fn same_union(annotated: &str, inferred: &str) -> bool {
    let (ours, theirs) = (union_members(annotated), union_members(inferred));
    ours.iter().all(|member| theirs.contains(member))
        && theirs.iter().all(|member| ours.contains(member))
}

fn literal_member(text: &str) -> Option<Lit> {
    let Some(quote) = text.chars().next().filter(|c| matches!(c, '\'' | '"')) else {
        return text.parse().ok().map(Lit::Int);
    };
    let inner = text.strip_prefix(quote)?.strip_suffix(quote)?;
    let (mut out, mut chars) = (String::new(), inner.chars());
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => out.push(other),
        }
    }
    Some(Lit::Str(out))
}

fn collect_literals(
    ty: &str,
    values: &mut Vec<Lit>,
    optional: &mut bool,
    found: &mut bool,
) -> Option<()> {
    for part in split_top(ty, '|') {
        if part == "None" {
            *optional = true;
        } else if let Some(inner) = part
            .strip_prefix("Optional[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            *optional = true;
            collect_literals(inner, values, optional, found)?;
        } else {
            let inner = part
                .strip_suffix(']')?
                .split_once('[')
                .filter(|(head, _)| head.rsplit('.').next() == Some("Literal"))?
                .1;
            for item in split_top(inner, ',') {
                let item = literal_member(item)?;
                if !values.contains(&item) {
                    values.push(item);
                }
            }
            *found = true;
        }
    }
    Some(())
}

/// The values of a `Literal` annotation or inferred type, and whether it is also `None`:
/// `Literal['a', "b"]`, `typing.Literal[1] | None`, `Optional[Literal['a']]`. Quotes,
/// order, repeats and the module spelling do not matter. `None` for any other type.
fn literal_set(ty: &str) -> Option<(Vec<Lit>, bool)> {
    let (mut values, mut optional, mut found) = (Vec::new(), false, false);
    collect_literals(ty, &mut values, &mut optional, &mut found)?;
    found.then_some((values, optional))
}

/// An annotated item and an inferred item that are both `Literal`s.
struct LiteralPair {
    annotated: Vec<Lit>,
    inferred: Vec<Lit>,
    same_nullability: bool,
}

fn literal_pair(annotated: &str, inferred: &str) -> Option<LiteralPair> {
    let (annotated, a_none) = literal_set(annotated)?;
    let (inferred, i_none) = literal_set(inferred)?;
    Some(LiteralPair {
        annotated,
        inferred,
        same_nullability: a_none == i_none,
    })
}

/// `literal-mismatch` and `redundant-literal` for the `Literal` items of a `tuple[...]`
/// annotation, against the inferred row type. Items whose nullability differs are
/// `stale-type`'s.
fn literal_findings(annotated: &str, inferred: &str) -> Vec<(Rule, String)> {
    let (Some(ours), Some(theirs)) = (tuple_items(annotated), tuple_items(inferred)) else {
        return Vec::new();
    };
    if ours.len() != theirs.len() {
        return Vec::new();
    }
    let show = |values: &[&Lit]| {
        values
            .iter()
            .map(|v| v.show())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut found = Vec::new();
    for (at, (a, i)) in ours.iter().zip(&theirs).enumerate() {
        let Some(pair) = literal_pair(a, i).filter(|pair| pair.same_nullability) else {
            continue;
        };
        let missing: Vec<&Lit> = pair
            .inferred
            .iter()
            .filter(|v| !pair.annotated.contains(v))
            .collect();
        let extra: Vec<&Lit> = pair
            .annotated
            .iter()
            .filter(|v| !pair.inferred.contains(v))
            .collect();
        if !missing.is_empty() {
            found.push((
                Rule::LiteralMismatch,
                format!(
                    "SQL[{annotated}] item {at} is {a} but its query column can be {}",
                    show(&missing)
                ),
            ));
        } else if !extra.is_empty() {
            found.push((
                Rule::RedundantLiteral,
                format!(
                    "SQL[{annotated}] item {at} is {a} but its query column is never {}",
                    show(&extra)
                ),
            ));
        }
    }
    found
}

/// Whether the annotation `a` is the `str` or `int` that an inferred `typing.Literal[...]`
/// item `i` (optionally `| None`) is a subtype of: less precise, but not stale.
fn covers_literal(a: &str, i: &str) -> bool {
    let (core, optional) = match i.strip_suffix(" | None") {
        Some(core) => (core, true),
        None => (i, false),
    };
    let Some(values) = core.strip_prefix("typing.Literal[") else {
        return false;
    };
    let base = if values.starts_with('\'') {
        "str"
    } else {
        "int"
    };
    a == base || (optional && a.strip_suffix(" | None") == Some(base))
}

/// Whether `annotated` is `inferred`, narrows a `typing.Any` item to any type, or
/// parametrises some of its generics (`tuple[int, list[str]]` for `tuple[int, list]`
/// or `tuple[int, list[int]]`), or is the `str` / `int` of an inferred `Literal`, so it is
/// not stale.
///
/// Element types are not checked.
pub(crate) fn refines_tuple(annotated: &str, inferred: &str) -> bool {
    fn balanced(args: &str) -> bool {
        let mut depth = 0i32;
        args.bytes().all(|byte| {
            depth += match byte {
                b'[' => 1,
                b']' => -1,
                _ => 0,
            };
            depth >= 0
        }) && depth == 0
    }
    match (tuple_items(annotated), tuple_items(inferred)) {
        (Some(ours), Some(theirs)) if ours.len() == theirs.len() => {
            ours.iter().zip(&theirs).all(|(a, i)| {
                let base = i.split('[').next().unwrap_or(i);
                a == i
                    || same_union(a, i)
                    || covers_literal(a, i)
                    || literal_pair(a, i).is_some_and(|pair| pair.same_nullability)
                    || *i == "typing.Any"
                    || (["list", "dict", "set", "frozenset", "tuple"].contains(&base)
                        && a.strip_prefix(base)
                            .and_then(|rest| rest.strip_prefix('['))
                            .and_then(|rest| rest.strip_suffix(']'))
                            .is_some_and(balanced))
            })
        }
        _ => false,
    }
}

/// Whether every row of the inferred `tuple[...]` is a row of the annotated one: `str | None`
/// admits `str`, `str` admits `Literal['a']`, and `object` or `typing.Any` admit anything.
fn admits_tuple(annotated: &str, inferred: &str) -> bool {
    fn literal_class(member: &str) -> Option<&'static str> {
        let values = member
            .strip_prefix("typing.")
            .unwrap_or(member)
            .strip_prefix("Literal[")?
            .strip_suffix(']')?;
        let values = split_top(values, ',');
        if values.iter().all(|v| v.starts_with(['\'', '"'])) {
            Some("str")
        } else if values.iter().all(|v| matches!(*v, "True" | "False")) {
            Some("bool")
        } else if values.iter().all(|v| v.parse::<i64>().is_ok()) {
            Some("int")
        } else {
            None
        }
    }
    fn admits(annotated: &str, inferred: &str) -> bool {
        let members = union_members(annotated);
        members
            .iter()
            .any(|m| matches!(*m, "typing.Any" | "Any" | "object"))
            || split_top(inferred, '|').iter().all(|member| {
                members.contains(member)
                    || literal_class(member).is_some_and(|class| members.contains(&class))
            })
    }
    match (tuple_items(annotated), tuple_items(inferred)) {
        (Some(ours), Some(theirs)) => match ours.as_slice() {
            [each, "..."] => theirs.iter().all(|i| admits(each, i)),
            _ => ours.len() == theirs.len() && ours.iter().zip(&theirs).all(|(a, i)| admits(a, i)),
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlty_sql::analyzer::SchemaData;
    use std::collections::HashMap;

    fn check_source(
        source: &str,
        dialect: &str,
        schema: Option<&SchemaData>,
        rules: &RuleConfig,
        style: Option<&str>,
        untyped: bool,
        any_calls: &[usize],
    ) -> Vec<Diagnostic> {
        let options = Options {
            dialect: dialect.parse().unwrap(),
            schema,
            style: ImportStyle::or_default(style),
            untyped,
        };
        super::check_source(source, &options, rules, any_calls)
    }

    fn override_rule(rule: &str, severity: &str) -> RuleConfig {
        let overrides = HashMap::from([(rule.to_string(), severity.to_string())]);
        RuleConfig::from_overrides(Some(overrides)).unwrap()
    }

    #[test]
    fn test_detects_stale_rewrite() {
        let source = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, Rule::StaleRewrite);
    }

    #[test]
    fn test_stale_rewrite_agrees_with_rewrite() {
        for (source, line) in [
            ("q = \"\"\"\n--sql\nSELECT 1::INTEGER AS n\"\"\"", 1),
            (
                "x = 1\nq = SQL[tuple[int]](\"--sql\\nSELECT 1\"), \"--sql\\nSELECT 2\"",
                2,
            ),
            ("q = '  --sql\\nSELECT 1'", 1),
        ] {
            let diags = check_source(
                source,
                "postgres",
                None,
                &RuleConfig::default(),
                None,
                false,
                &[],
            );
            let lines: Vec<usize> = diags
                .iter()
                .filter(|d| d.rule == Rule::StaleRewrite)
                .map(|d| d.line)
                .collect();
            assert_eq!(lines, vec![line], "{source}");
        }
    }

    #[test]
    fn test_unwrappable_string_is_not_stale() {
        for source in [
            r#"q = "--sql SELECT 1::INTEGER""#,
            r#"q = "--sql\nDROP TABLE t""#,
        ] {
            let diags = check_source(
                source,
                "postgres",
                None,
                &RuleConfig::default(),
                None,
                false,
                &[],
            );
            assert!(
                diags.iter().all(|d| d.rule != Rule::StaleRewrite),
                "{source}"
            );
        }
    }

    #[test]
    fn test_untyped_rows_are_stale_with_sentinel() {
        let source = r#"q = "--sql\nSELECT x, y FROM pairs""#;
        assert!(check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[]
        )
        .is_empty());
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            true,
            &[],
        );
        assert_eq!(diags[0].rule, Rule::StaleRewrite);
    }

    #[test]
    fn test_clean_no_stale() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        let stale_rewrites: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::StaleRewrite)
            .collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_stale_type() {
        let source = r#"q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        let narrow: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::NarrowType)
            .collect();
        assert_eq!(narrow.len(), 1);
        assert!(narrow[0].message.contains("tuple[int]"));
    }

    #[test]
    fn test_wider_annotation_is_stale_not_narrow() {
        for (annotated, inferred, admitted) in [
            ("tuple[str | None]", "tuple[str]", true),
            ("tuple[object, typing.Any]", "tuple[int | None, str]", true),
            (
                "tuple[str, int, bool]",
                "tuple[Literal['a', \"b\"], typing.Literal[-1, 2], Literal[True]]",
                true,
            ),
            ("tuple[str]", "tuple[str | None]", false),
            ("tuple[int]", "tuple[int, str]", false),
            ("tuple[int]", "tuple[Literal['a']]", false),
            ("tuple[int]", "dict[str, int]", false),
        ] {
            assert_eq!(
                admits_tuple(annotated, inferred),
                admitted,
                "{annotated} {inferred}"
            );
        }
    }

    #[test]
    fn test_parametrised_tuple_item_is_not_stale() {
        let source =
            r#"q = SQL[tuple[int, list[str]]]("--sql\nSELECT 1::INTEGER, t::TEXT[] FROM t")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        assert!(diags.iter().all(|d| d.rule != Rule::StaleType));
        assert!(refines_tuple("tuple[int, list[str]]", "tuple[int, list]"));
        assert!(!refines_tuple("tuple[str, list[str]]", "tuple[int, list]"));
        assert!(!refines_tuple("tuple[int, list[str]]", "tuple[int, li]"));
        assert!(!refines_tuple("tuple[int[str]]", "tuple[int]"));
        assert!(!refines_tuple("tuple[[int]]", "tuple[]"));
        assert!(!refines_tuple(
            "tuple[int, list[str] | str]",
            "tuple[int, list]"
        ));
        assert!(!refines_tuple(
            "tuple[list[str] | list[int]]",
            "tuple[list]"
        ));
        assert!(!refines_tuple(
            "tuple[int, list[str] | None]",
            "tuple[int, list]"
        ));
        assert!(!refines_tuple("tuple[int, list[str]]", "tuple[int]"));
        assert!(!refines_tuple("list[str]", "list"));
        assert!(refines_tuple(
            "tuple[dict[str, list[int]], int]",
            "tuple[dict, int]"
        ));
        assert!(refines_tuple("tuple[list[list[int]]]", "tuple[list]"));
        assert!(refines_tuple("tuple[list[str]]", "tuple[list[int]]"));
        assert!(!refines_tuple("tuple[set[str]]", "tuple[list[int]]"));
        assert!(refines_tuple(
            "tuple[list[str], dict]",
            "tuple[typing.Any, dict]"
        ));
        assert!(!refines_tuple("tuple[int]", "tuple[typing.Any, dict]"));
    }

    #[test]
    fn test_rule_override_ignore() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let rules = override_rule("stale-rewrite", "ignore");
        let diags = check_source(source, "postgres", None, &rules, None, false, &[]);
        let stale_rewrites: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::StaleRewrite)
            .collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_unwrapped() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::InlineSqlComment)
            .collect();
        assert_eq!(inline.len(), 1);
        assert!(inline[0].message.contains("SQL comment"));
    }

    #[test]
    fn test_no_inline_sql_comment_with_newline() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::InlineSqlComment)
            .collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_wrapped() {
        let source = r#"q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::InlineSqlComment)
            .collect();
        assert_eq!(inline.len(), 1);
    }

    #[test]
    fn test_inline_sql_comment_ignored_when_disabled() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let rules = override_rule("inline-sql-comment", "ignore");
        let diags = check_source(source, "postgres", None, &rules, None, false, &[]);
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::InlineSqlComment)
            .collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_check_with_alias_import_style() {
        let source = r#"q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            Some("from sqlty import SQL as Q"),
            false,
            &[],
        );
        let stale_rewrites: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::StaleRewrite)
            .collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_check_with_module_import_style() {
        let source = r#"q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            Some("import sqlty"),
            false,
            &[],
        );
        let stale_rewrites: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == Rule::StaleRewrite)
            .collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_name_rules_are_on_by_default() {
        let schema = Some(sqlty_sql::analyzer::load_schema_with_skipped(
            "CREATE TABLE users (id INT)",
            "postgres",
        ));
        let schema = schema.map(|(schema, _)| schema);
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT nope FROM users")"#;
        assert!(!check_source(
            source,
            "postgres",
            schema.as_deref(),
            &RuleConfig::default(),
            None,
            false,
            &[]
        )
        .is_empty());
        let rules = override_rule("unknown-column", "error");
        let diags = check_source(
            source,
            "postgres",
            schema.as_deref(),
            &rules,
            None,
            false,
            &[],
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, Rule::Name(NameRule::UnknownColumn));
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn test_untyped_query() {
        let source = r#"q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, Rule::UntypedQuery);
        assert!(diags[0].message.contains("add casts"));
    }

    #[test]
    fn test_untyped_query_now_inferable() {
        let source = r#"q = sqlty.SQL[sqlty.UNTYPED]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            Some("import sqlty"),
            false,
            &[],
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "sqlty.SQL[sqlty.UNTYPED] can now be sqlty.SQL[tuple[int]]; run sqlty check --fix"
        );
    }

    #[test]
    fn test_any_calls_skip_unchecked_query() {
        let source = "a = SQL[A](\"--sql\\nSELECT 1\" + x)\nb = SQL[A](r\"--sql\\nSELECT 1\")\nc = SQL[A](\"--sql\\nSELECT 1\" + x)\n";
        let any: Vec<usize> = source
            .match_indices("SQL")
            .map(|(i, _)| i)
            .take(2)
            .collect();
        let lines = |diags: Vec<Diagnostic>| diags.into_iter().map(|d| d.line).collect::<Vec<_>>();
        assert_eq!(
            lines(check_source(
                source,
                "postgres",
                None,
                &RuleConfig::default(),
                None,
                false,
                &any
            )),
            [3]
        );
        assert_eq!(
            lines(check_source(
                source,
                "postgres",
                None,
                &RuleConfig::default(),
                None,
                false,
                &[]
            )),
            [1, 2, 3]
        );
        let sites = query_sites(source, &ImportStyle::default(), &any);
        assert_eq!(sites.iter().map(|s| s.0).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn test_skipped_and_unchecked_queries() {
        let source = "a = (\"--sql\\nSELECT 1 \"\n     f\"AS n{x}\")\nb = SQL[T](\"--sql\\nSELECT 1\" + x)\nc = r\"--sql\\nSELECT 1\"\n";
        let diags: Vec<(Rule, usize)> = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        )
        .into_iter()
        .map(|d| (d.rule, d.line))
        .collect();
        assert_eq!(
            diags,
            vec![
                (Rule::FstringSkipped, 1),
                (Rule::UncheckedQuery, 3),
                (Rule::UncheckedQuery, 4),
            ]
        );
    }

    #[test]
    fn test_every_wrapper_on_a_line() {
        let source = "a, b = SQL[tuple[int]](\"--sql\\nSELECT 1\"), SQL[tuple[int]](\"--sql\\nSELECT 'x'::TEXT\")";
        let diags = check_source(
            source,
            "postgres",
            None,
            &RuleConfig::default(),
            None,
            false,
            &[],
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "SQL[tuple[int]] should be SQL[tuple[str]]"
        );
    }

    #[test]
    fn test_wrapper_calls_span_lines() {
        let source = r#"q = SQL[tuple[str]]("""--sql
SELECT 1""")
r = SQL[T](
    "-- sql\nSELECT 2",
)
s = SQL[T]("plain")
"#;
        let queries: Vec<(usize, Option<String>)> =
            wrapped_queries(source, &ImportStyle::default())
                .into_iter()
                .map(|(line, _, query)| (line, query))
                .collect();
        assert_eq!(
            queries,
            vec![
                (1, Some("SELECT 1".to_string())),
                (3, Some("SELECT 2".to_string())),
                (6, None),
            ]
        );
    }
}
