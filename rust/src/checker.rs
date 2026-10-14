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
//!   unless its wrapper's `T` is one of `any_annotations`

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{parse_error, Schema};
use crate::rewriter::infer_sql_content as infer_from_sql_content;
use crate::rewriter::{
    after_marker, ends_marker_line, extract_string_content, marked_query, skipped_rule,
    string_literals, strip_line_break, wraps, ImportStyle, LineIndex,
};
use crate::tokens::{content, string_groups, tokenize, wrapper_calls};
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
pub(crate) static INLINE_SQL_COMMENT_CONTENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!("^{INLINE_SQL_COMMENT}")).unwrap());

/// Lines of plain strings opening with a `--sql` marker line, which MySQL and
/// MariaDB reject: they read `--` as a comment only before whitespace.
fn mysql_marker_lines(source: &str, lines: &LineIndex) -> Vec<usize> {
    let mut lines: Vec<usize> = string_literals(source)
        .into_iter()
        .filter(|&(start, end)| {
            extract_string_content(&source[start..end])
                .trim_start()
                .strip_prefix("--sql")
                .is_some_and(ends_marker_line)
        })
        .map(|(start, _)| lines.line_at(start))
        .collect();
    lines.dedup();
    lines
}

/// A diagnostic result from checking source code.
pub struct Diagnostic {
    pub rule: String,
    pub message: String,
    pub severity: String,
    pub line: usize,
}

/// Rule severity configuration.
pub struct RuleConfig {
    overrides: HashMap<String, String>,
}

impl RuleConfig {
    pub fn new(overrides: Option<HashMap<String, String>>) -> Self {
        RuleConfig {
            overrides: overrides.unwrap_or_default(),
        }
    }

    /// Get the effective severity for a rule.
    pub fn severity(&self, rule: &str) -> &str {
        if let Some(override_val) = self.overrides.get(rule) {
            return override_val.as_str();
        }
        // Defaults
        match rule {
            "stale-rewrite" => "warn",
            "stale-type" => "warn",
            "select-star" => "warn",
            "select-star-no-schema" => "error",
            "column-count-mismatch" => "error",
            "column-type-mismatch" => "error",
            "column-name-mismatch" => "warn",
            "typeddict-key-mismatch" => "error",
            "typeddict-inheritance" => "error",
            "inline-sql-comment" => "warn",
            "untyped-query" => "warn",
            "unparsed-query" => "ignore",
            _ => "warn",
        }
    }

    /// Check if a rule is enabled (not "ignore").
    pub fn is_enabled(&self, rule: &str) -> bool {
        self.severity(rule) != "ignore"
    }
}

/// Check Python source code for SQL type issues.
pub fn check_source(
    source: &str,
    dialect: &str,
    schema: &Option<Schema>,
    rules: Option<HashMap<String, String>>,
    import_style: Option<&str>,
    untyped: bool,
    any_annotations: &[String],
) -> Vec<Diagnostic> {
    let rule_config = RuleConfig::new(rules);
    let unparsed_enabled = rule_config.is_enabled("unparsed-query");
    let has_schema = schema.is_some();
    let mut diagnostics = Vec::new();
    let mut report = |rule: &str, message: String, line: usize| {
        if rule_config.is_enabled(rule) {
            diagnostics.push(Diagnostic {
                rule: rule.to_string(),
                message,
                severity: rule_config.severity(rule).to_string(),
                line,
            });
        }
    };

    let style = ImportStyle::or_default(import_style);
    let (wrapper_name, untyped_name) = (style.wrapper_name.clone(), style.untyped_name());
    let lines = LineIndex::new(source);
    if dialect.eq_ignore_ascii_case("mysql") {
        for line in mysql_marker_lines(source, &lines) {
            report(
                "mysql-sql-marker",
                r#"MySQL reads "--sql" as an expression, not a comment; use "-- sql""#.to_string(),
                line,
            );
        }
    }

    if rule_config.is_enabled("stale-rewrite") {
        for (start, _, _) in wraps(source, dialect, schema, &style, untyped, &|_| false) {
            report(
                "stale-rewrite",
                "Source has --sql strings that aren't wrapped in SQL[T]".to_string(),
                lines.line_at(start),
            );
        }
    }

    let tokens = tokenize(source);
    let calls = wrapper_calls(source, &tokens, &wrapper_name);
    for group in string_groups(source, &tokens) {
        let first = &tokens[group[0]];
        let line = lines.line_at(first.span.start);
        let enclosing = calls
            .iter()
            .find(|call| call.span.start <= first.span.start && first.span.end <= call.span.end);
        if let Some(rule) = skipped_rule(source, &tokens, &group) {
            if !(rule == "unchecked-query"
                && enclosing.is_some_and(|call| call.is_any(source, any_annotations)))
            {
                report(rule, skipped_message(rule), line);
            }
            continue;
        }
        let spans: Vec<Range<usize>> = group.iter().map(|&i| tokens[i].span.clone()).collect();
        if enclosing.is_some() {
            continue;
        }
        let sql_content = content(source, &spans);
        if INLINE_SQL_COMMENT_CONTENT.is_match(&sql_content) {
            report("inline-sql-comment", INLINE_MESSAGE.to_string(), line);
        }
        if let Some(error) = marked_query(&sql_content)
            .filter(|_| unparsed_enabled)
            .and_then(|sql| parse_error(&sql, dialect))
        {
            report("unparsed-query", unparsed_message(&error), line);
        }
    }

    for call in &calls {
        let line_num = lines.line_at(call.span.start);
        let Some(sql_content) = call.content(source) else {
            let interpolated = call.strings.iter().any(|span| {
                source[span.clone()]
                    .split(['"', '\''])
                    .next()
                    .is_some_and(|prefix| prefix.contains(['f', 'F', 't', 'T']))
            });
            if !interpolated
                && !call.is_any(source, any_annotations)
                && after_marker(&content(source, &call.strings)).is_some()
            {
                report(
                    "unchecked-query",
                    skipped_message("unchecked-query"),
                    line_num,
                );
            }
            continue;
        };
        if after_marker(&sql_content).is_none() {
            continue;
        }
        if INLINE_SQL_COMMENT_CONTENT.is_match(&sql_content) {
            report("inline-sql-comment", INLINE_MESSAGE.to_string(), line_num);
        }
        if let Some(error) = marked_query(&sql_content)
            .filter(|_| unparsed_enabled)
            .and_then(|sql| parse_error(&sql, dialect))
        {
            report("unparsed-query", unparsed_message(&error), line_num);
        }

        let upper = sql_content.to_uppercase();
        if upper.contains("SELECT *") || upper.contains("SELECT\n*") {
            if has_schema {
                report(
                    "select-star",
                    "SELECT * used — expand to explicit column list (joins need a manual edit)"
                        .to_string(),
                    line_num,
                );
            } else {
                report(
                    "select-star-no-schema",
                    "SELECT * used but no schema available".to_string(),
                    line_num,
                );
            }
        }

        let annotated = &source[call.annotation.clone()];
        if annotated == untyped_name {
            let untyped = format!("{}[{}]", wrapper_name, untyped_name);
            // A `*` the schema cannot resolve infers no type, so none is guessed.
            let message = match infer_from_sql_content(&sql_content, dialect, schema) {
                Some(current) => format!(
                    "{} can now be {}[{}]; run sqlty check --fix",
                    untyped, wrapper_name, current
                ),
                None => format!(
                    "{} has no inferable row type; add casts (e.g. id::INTEGER) or a schema",
                    untyped
                ),
            };
            report("untyped-query", message, line_num);
            continue;
        }

        if annotated.starts_with("tuple[") && rule_config.is_enabled("stale-type") {
            if let Some(current) = infer_from_sql_content(&sql_content, dialect, schema) {
                if !refines_tuple(annotated, &current) {
                    report(
                        "stale-type",
                        format!("SQL[{}] should be SQL[{}]", annotated, current),
                        line_num,
                    );
                }
            }
        }
    }

    diagnostics.sort_by_key(|d| d.line);
    diagnostics.dedup_by(|a, b| a.line == b.line && a.rule == b.rule && a.message == b.message);
    diagnostics
}

const INLINE_MESSAGE: &str = r#""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"#;

fn skipped_message(rule: &str) -> String {
    match rule {
        "tstring-skipped" => {
            "t-string with --sql prefix skipped (t-strings cannot be statically rewritten)"
        }
        "fstring-skipped" => {
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

/// Each query `sqlty check` sees, as `(line, kind, text)` for `--statistics`: `kind` is
/// `query` with the marked text, or the rule that skips the string (text empty).
pub fn query_sites(
    source: &str,
    import_style: Option<&str>,
    any_annotations: &[String],
) -> Vec<(usize, &'static str, String)> {
    let wrapper_name = ImportStyle::or_default(import_style).wrapper_name;
    let lines = LineIndex::new(source);
    let tokens = tokenize(source);
    let calls = wrapper_calls(source, &tokens, &wrapper_name);
    let mut sites = Vec::new();
    let mut skipped: Vec<Range<usize>> = Vec::new();
    for group in string_groups(source, &tokens) {
        let first = &tokens[group[0]];
        let line = lines.line_at(first.span.start);
        let enclosing = calls
            .iter()
            .find(|call| call.span.start <= first.span.start && first.span.end <= call.span.end);
        if let Some(rule) = skipped_rule(source, &tokens, &group) {
            let last = &tokens[group[group.len() - 1]];
            skipped.push(first.span.start..last.span.end);
            if !(rule == "unchecked-query"
                && enclosing.is_some_and(|call| call.is_any(source, any_annotations)))
            {
                sites.push((line, rule, String::new()));
            }
            continue;
        }
        if enclosing.is_some() {
            continue;
        }
        let spans: Vec<Range<usize>> = group.iter().map(|&i| tokens[i].span.clone()).collect();
        if let Some(sql) = marked_query(&content(source, &spans)) {
            sites.push((line, "query", sql));
        }
    }
    for call in &calls {
        let line = lines.line_at(call.span.start);
        match call.content(source) {
            Some(text) => {
                if let Some(sql) = marked_query(&text) {
                    sites.push((line, "query", sql));
                }
            }
            None if after_marker(&content(source, &call.strings)).is_some()
                && !call.is_any(source, any_annotations)
                && !skipped.iter().any(|span| call.span.contains(&span.start)) =>
            {
                sites.push((line, "unchecked-query", String::new()));
            }
            None => {}
        }
    }
    sites.sort_by_key(|site| site.0);
    sites
}

/// `(line, annotation, query)` for each `{wrapper}[T](...)` call; `query` is the string
/// without its marker, `None` unless the call holds a marked string literal.
pub fn wrapped_queries(
    source: &str,
    import_style: Option<&str>,
) -> Vec<(usize, String, Option<String>)> {
    let wrapper_name = ImportStyle::or_default(import_style).wrapper_name;
    let lines = LineIndex::new(source);
    wrapper_calls(source, &tokenize(source), &wrapper_name)
        .into_iter()
        .map(|call| {
            let query = call.content(source).and_then(|content| {
                let rest = after_marker(&content)?;
                Some(strip_line_break(rest).unwrap_or(rest).trim().to_string())
            });
            (
                lines.line_at(call.span.start),
                source[call.annotation.clone()].to_string(),
                query,
            )
        })
        .collect()
}

/// Whether `annotated` is `inferred`, narrows a `typing.Any` item to any type, or
/// parametrises some of its generics (`tuple[int, list[str]]` for `tuple[int, list]`
/// or `tuple[int, list[int]]`), so it is not stale.
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
    fn items(ty: &str) -> Option<Vec<&str>> {
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
    match (items(annotated), items(inferred)) {
        (Some(ours), Some(theirs)) if ours.len() == theirs.len() => {
            ours.iter().zip(&theirs).all(|(a, i)| {
                let base = i.split('[').next().unwrap_or(i);
                a == i
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_stale_rewrite() {
        let source = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "stale-rewrite");
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
            let diags = check_source(source, "postgres", &None, None, None, false, &[]);
            let lines: Vec<usize> = diags
                .iter()
                .filter(|d| d.rule == "stale-rewrite")
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
            let diags = check_source(source, "postgres", &None, None, None, false, &[]);
            assert!(diags.iter().all(|d| d.rule != "stale-rewrite"), "{source}");
        }
    }

    #[test]
    fn test_untyped_rows_are_stale_with_sentinel() {
        let source = r#"q = "--sql\nVALUES (1, 2)""#;
        assert!(check_source(source, "postgres", &None, None, None, false, &[]).is_empty());
        let diags = check_source(source, "postgres", &None, None, None, true, &[]);
        assert_eq!(diags[0].rule, "stale-rewrite");
    }

    #[test]
    fn test_clean_no_stale() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_stale_type() {
        let source = r#"q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")"#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        let stale_types: Vec<_> = diags.iter().filter(|d| d.rule == "stale-type").collect();
        assert_eq!(stale_types.len(), 1);
        assert!(stale_types[0].message.contains("tuple[int]"));
    }

    #[test]
    fn test_parametrised_tuple_item_is_not_stale() {
        let source =
            r#"q = SQL[tuple[int, list[str]]]("--sql\nSELECT 1::INTEGER, t::TEXT[] FROM t")"#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        assert!(diags.iter().all(|d| d.rule != "stale-type"));
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
        let mut rules = HashMap::new();
        rules.insert("stale-rewrite".to_string(), "ignore".to_string());
        let diags = check_source(source, "postgres", &None, Some(rules), None, false, &[]);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_unwrapped() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "inline-sql-comment")
            .collect();
        assert_eq!(inline.len(), 1);
        assert!(inline[0].message.contains("SQL comment"));
    }

    #[test]
    fn test_no_inline_sql_comment_with_newline() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "inline-sql-comment")
            .collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_wrapped() {
        let source = r#"q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "inline-sql-comment")
            .collect();
        assert_eq!(inline.len(), 1);
    }

    #[test]
    fn test_inline_sql_comment_ignored_when_disabled() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let mut rules = HashMap::new();
        rules.insert("inline-sql-comment".to_string(), "ignore".to_string());
        let diags = check_source(source, "postgres", &None, Some(rules), None, false, &[]);
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "inline-sql-comment")
            .collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_check_with_alias_import_style() {
        let source = r#"q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            &None,
            None,
            Some("from sqlty import SQL as Q"),
            false,
            &[],
        );
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_check_with_module_import_style() {
        let source = r#"q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            &None,
            None,
            Some("import sqlty"),
            false,
            &[],
        );
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_untyped_query() {
        let source = r#"q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")"#;
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "untyped-query");
        assert!(diags[0].message.contains("add casts"));
    }

    #[test]
    fn test_untyped_query_now_inferable() {
        let source = r#"q = sqlty.SQL[sqlty.UNTYPED]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(
            source,
            "postgres",
            &None,
            None,
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
    fn test_any_annotations_skip_unchecked_query() {
        let source = "a = SQL[A](\"--sql\\nSELECT 1\" + x)\nb = SQL[A](r\"--sql\\nSELECT 1\")\nc = SQL[B](\"--sql\\nSELECT 1\" + x)\n";
        let any = ["A".to_string()];
        let lines = |diags: Vec<Diagnostic>| diags.into_iter().map(|d| d.line).collect::<Vec<_>>();
        assert_eq!(
            lines(check_source(
                source, "postgres", &None, None, None, false, &any
            )),
            [3]
        );
        assert_eq!(
            lines(check_source(
                source,
                "postgres",
                &None,
                None,
                None,
                false,
                &[]
            )),
            [1, 2, 3]
        );
        let sites = query_sites(source, None, &any);
        assert_eq!(sites.iter().map(|s| s.0).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn test_skipped_and_unchecked_queries() {
        let source = "a = (\"--sql\\nSELECT 1 \"\n     f\"AS n{x}\")\nb = SQL[T](\"--sql\\nSELECT 1\" + x)\nc = r\"--sql\\nSELECT 1\"\n";
        let diags: Vec<(String, usize)> =
            check_source(source, "postgres", &None, None, None, false, &[])
                .into_iter()
                .map(|d| (d.rule, d.line))
                .collect();
        assert_eq!(
            diags,
            vec![
                ("fstring-skipped".to_string(), 1),
                ("unchecked-query".to_string(), 3),
                ("unchecked-query".to_string(), 4),
            ]
        );
    }

    #[test]
    fn test_every_wrapper_on_a_line() {
        let source = "a, b = SQL[tuple[int]](\"--sql\\nSELECT 1\"), SQL[tuple[int]](\"--sql\\nSELECT 'x'::TEXT\")";
        let diags = check_source(source, "postgres", &None, None, None, false, &[]);
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
        let queries: Vec<(usize, Option<String>)> = wrapped_queries(source, None)
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
