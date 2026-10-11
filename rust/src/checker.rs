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

use std::collections::HashMap;

use regex::Regex;

use crate::analyzer::Schema;
use crate::rewriter::infer_sql_content as infer_from_sql_content;
use crate::rewriter::{header_end, rewrite_source, skipped_string_rule};

/// Pattern to detect unwrapped --sql strings.
fn unwrapped_sql_pattern() -> Regex {
    Regex::new(r#"["']--sql(\s|\\n)"#).unwrap()
}

/// Pattern to detect --sql followed by a space (not \n) — the query becomes a SQL comment.
fn inline_sql_comment_pattern() -> Regex {
    Regex::new(r#"["']--sql [^\\]"#).unwrap()
}

/// Build a regex to extract SQL content from `{wrapper_name}[T]("--sql ...")`.
fn build_sql_content_pattern(wrapper_name: &str) -> Regex {
    let escaped = regex::escape(wrapper_name);
    Regex::new(&format!(r#"{}\[.*?\]\(\s*["'](.+?)["']\s*\)"#, escaped)).unwrap()
}

/// Build a regex to extract the type annotation from `{wrapper_name}[T](...)`.
fn build_type_annotation_pattern(wrapper_name: &str) -> Regex {
    let escaped = regex::escape(wrapper_name);
    Regex::new(&format!(r"{}\[(.+?)\]\(", escaped)).unwrap()
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
            _ => "warn",
        }
    }

    /// Check if a rule is enabled (not "ignore").
    pub fn is_enabled(&self, rule: &str) -> bool {
        self.severity(rule) != "ignore"
    }
}

/// Parse import-style into wrapper_name.
fn parse_wrapper_name(import_style: Option<&str>) -> String {
    match import_style {
        None | Some("from sqlty import SQL") => "SQL".to_string(),
        Some(s) if s.starts_with("from sqlty import SQL as ") => {
            s["from sqlty import SQL as ".len()..].trim().to_string()
        }
        Some("import sqlty") => "sqlty.SQL".to_string(),
        Some(s) if s.starts_with("import sqlty as ") => {
            let alias = s["import sqlty as ".len()..].trim();
            format!("{}.SQL", alias)
        }
        Some(_) => "SQL".to_string(),
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
) -> Vec<Diagnostic> {
    let rule_config = RuleConfig::new(rules);
    let has_schema = schema.is_some();
    let mut diagnostics = Vec::new();

    let wrapper_name = parse_wrapper_name(import_style);
    let wrapper_marker = format!("{}[", wrapper_name);
    let untyped_name = match wrapper_name.rsplit_once('.') {
        Some((module, _)) => format!("{}.UNTYPED", module),
        None => "UNTYPED".to_string(),
    };
    let sql_content_pat = build_sql_content_pattern(&wrapper_name);
    let type_annotation_pat = build_type_annotation_pattern(&wrapper_name);
    let unwrapped_pat = unwrapped_sql_pattern();
    let inline_pat = inline_sql_comment_pattern();

    let wrappable = if rule_config.is_enabled("stale-rewrite") {
        wrappable_lines(source, dialect, schema, import_style, untyped)
    } else {
        Vec::new()
    };

    for (line_num, logical) in logical_lines(source, &wrapper_name) {
        let trimmed = logical.as_str();

        if let Some(rule) = skipped_string_rule(trimmed) {
            if rule_config.is_enabled(rule) {
                let kind = if rule == "tstring-skipped" {
                    "t-string"
                } else {
                    "f-string"
                };
                diagnostics.push(Diagnostic {
                    rule: rule.to_string(),
                    message: format!(
                        "{kind} with --sql prefix skipped ({kind}s cannot be statically rewritten)"
                    ),
                    severity: rule_config.severity(rule).to_string(),
                    line: line_num,
                });
            }
            continue;
        }

        // Check for unwrapped --sql strings (stale-rewrite)
        if is_unwrapped_sql_string(trimmed, &wrapper_marker, &unwrapped_pat) {
            if wrappable.contains(&line_num) && rule_config.is_enabled("stale-rewrite") {
                diagnostics.push(Diagnostic {
                    rule: "stale-rewrite".to_string(),
                    message: "Source has --sql strings that aren't wrapped in SQL[T]".to_string(),
                    severity: rule_config.severity("stale-rewrite").to_string(),
                    line: line_num,
                });
            }
            // Also check for inline-sql-comment on unwrapped strings
            if rule_config.is_enabled("inline-sql-comment")
                && has_inline_sql_comment(trimmed, &inline_pat)
            {
                diagnostics.push(Diagnostic {
                    rule: "inline-sql-comment".to_string(),
                    message: r#""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"#.to_string(),
                    severity: rule_config.severity("inline-sql-comment").to_string(),
                    line: line_num,
                });
            }
            continue;
        }

        // Check for SQL[T](...) wrappers
        if let Some(sql_content) = extract_sql_from_wrapper(trimmed, &sql_content_pat) {
            // Check inline-sql-comment on wrapped strings
            if rule_config.is_enabled("inline-sql-comment") && sql_content.starts_with("--sql ") {
                diagnostics.push(Diagnostic {
                    rule: "inline-sql-comment".to_string(),
                    message: r#""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"#.to_string(),
                    severity: rule_config.severity("inline-sql-comment").to_string(),
                    line: line_num,
                });
            }

            // Check SELECT *
            let upper = sql_content.to_uppercase();
            let has_star = upper.contains("SELECT *") || upper.contains("SELECT\n*");
            // Without a schema, `*` has no known columns, so its inferred type is a guess.
            let guessed = has_star && !has_schema;
            if has_star {
                if has_schema && rule_config.is_enabled("select-star") {
                    diagnostics.push(Diagnostic {
                        rule: "select-star".to_string(),
                        message: "SELECT * used — expand to explicit column list".to_string(),
                        severity: rule_config.severity("select-star").to_string(),
                        line: line_num,
                    });
                } else if !has_schema && rule_config.is_enabled("select-star-no-schema") {
                    diagnostics.push(Diagnostic {
                        rule: "select-star-no-schema".to_string(),
                        message: "SELECT * used but no schema available".to_string(),
                        severity: rule_config.severity("select-star-no-schema").to_string(),
                        line: line_num,
                    });
                }
            }

            let annotated = extract_type_from_wrapper(trimmed, &type_annotation_pat);
            if annotated.as_deref() == Some(untyped_name.as_str()) {
                if rule_config.is_enabled("untyped-query") {
                    let untyped = format!("{}[{}]", wrapper_name, untyped_name);
                    let current = if guessed {
                        None
                    } else {
                        infer_from_sql_content(&sql_content, dialect, schema)
                    };
                    let message = match current {
                        Some(current) => format!(
                            "{} can now be {}[{}]; run sqlty check --fix",
                            untyped, wrapper_name, current
                        ),
                        None => format!(
                            "{} has no inferable row type; add casts (e.g. id::INTEGER) or a schema",
                            untyped
                        ),
                    };
                    diagnostics.push(Diagnostic {
                        rule: "untyped-query".to_string(),
                        message,
                        severity: rule_config.severity("untyped-query").to_string(),
                        line: line_num,
                    });
                }
                continue;
            }

            // Check stale types
            if rule_config.is_enabled("stale-type") && !guessed {
                if let Some(annotated_type) = annotated.filter(|ty| ty.starts_with("tuple[")) {
                    let current_type = infer_from_sql_content(&sql_content, dialect, schema);
                    if let Some(ref current) = current_type {
                        if *current != annotated_type {
                            diagnostics.push(Diagnostic {
                                rule: "stale-type".to_string(),
                                message: format!(
                                    "SQL[{}] should be SQL[{}]",
                                    annotated_type, current
                                ),
                                severity: rule_config.severity("stale-type").to_string(),
                                line: line_num,
                            });
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

/// Check if a line has `"--sql SELECT ..."` (space after --sql, not \n).
fn has_inline_sql_comment(line: &str, pattern: &Regex) -> bool {
    pattern.is_match(line)
}

/// Lines holding a string that `rewrite` would wrap.
///
/// Rewrite only inserts imports at the header and wraps strings in place,
/// so each other line keeps its position, shifted past the imports.
fn wrappable_lines(
    source: &str,
    dialect: &str,
    schema: &Option<Schema>,
    import_style: Option<&str>,
    untyped: bool,
) -> Vec<usize> {
    let original: Vec<&str> = source.lines().collect();
    let rewritten_source = rewrite_source(source, dialect, schema, import_style, untyped);
    let rewritten: Vec<&str> = rewritten_source.lines().collect();
    let added = rewritten.len() - original.len();
    let head = source[..header_end(source)].matches('\n').count();
    original
        .iter()
        .enumerate()
        .filter(|(i, line)| rewritten[if *i < head { *i } else { i + added }] != **line)
        .map(|(i, _)| i + 1)
        .collect()
}

/// Stripped lines, with a wrapper split by a formatter joined onto its first line.
fn logical_lines(source: &str, wrapper_name: &str) -> Vec<(usize, String)> {
    let opener = Regex::new(&format!(r"{}\[.*\]\($", regex::escape(wrapper_name))).unwrap();
    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if opener.is_match(lines[i]) && i + 2 < lines.len() && lines[i + 2].starts_with(')') {
            let inner = lines[i + 1].trim_end_matches(',');
            result.push((i + 1, format!("{}{}{}", lines[i], inner, lines[i + 2])));
            i += 3;
            continue;
        }
        result.push((i + 1, lines[i].to_string()));
        i += 1;
    }
    result
}

/// Check if a line contains an unwrapped --sql string (not in wrapper).
fn is_unwrapped_sql_string(line: &str, wrapper_marker: &str, pattern: &Regex) -> bool {
    if line.contains(wrapper_marker) {
        return false;
    }
    pattern.is_match(line)
}

/// Extract SQL content from wrapper pattern.
fn extract_sql_from_wrapper(line: &str, pattern: &Regex) -> Option<String> {
    let cap = pattern.captures(line)?;
    let content = cap.get(1)?.as_str();
    if content.starts_with("--sql") {
        Some(content.to_string())
    } else {
        None
    }
}

/// Extract the type annotation from wrapper pattern.
fn extract_type_from_wrapper(line: &str, pattern: &Regex) -> Option<String> {
    let cap = pattern.captures(line)?;
    Some(cap.get(1)?.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_stale_rewrite() {
        let source = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None, None, false);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "stale-rewrite");
    }

    #[test]
    fn test_unwrappable_string_is_not_stale() {
        for source in [
            r#"q = "--sql SELECT 1::INTEGER""#,
            r#"q = "--sql\nDROP TABLE t""#,
        ] {
            let diags = check_source(source, "postgres", &None, None, None, false);
            assert!(diags.iter().all(|d| d.rule != "stale-rewrite"), "{source}");
        }
    }

    #[test]
    fn test_untyped_rows_are_stale_with_sentinel() {
        let source = r#"q = "--sql\nVALUES (1, 2)""#;
        assert!(check_source(source, "postgres", &None, None, None, false).is_empty());
        let diags = check_source(source, "postgres", &None, None, None, true);
        assert_eq!(diags[0].rule, "stale-rewrite");
    }

    #[test]
    fn test_clean_no_stale() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None, false);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_stale_type() {
        let source = r#"q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")"#;
        let diags = check_source(source, "postgres", &None, None, None, false);
        let stale_types: Vec<_> = diags.iter().filter(|d| d.rule == "stale-type").collect();
        assert_eq!(stale_types.len(), 1);
        assert!(stale_types[0].message.contains("tuple[int]"));
    }

    #[test]
    fn test_rule_override_ignore() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let mut rules = HashMap::new();
        rules.insert("stale-rewrite".to_string(), "ignore".to_string());
        let diags = check_source(source, "postgres", &None, Some(rules), None, false);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_unwrapped() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None, None, false);
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
        let diags = check_source(source, "postgres", &None, None, None, false);
        let inline: Vec<_> = diags
            .iter()
            .filter(|d| d.rule == "inline-sql-comment")
            .collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_wrapped() {
        let source = r#"q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None, false);
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
        let diags = check_source(source, "postgres", &None, Some(rules), None, false);
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
        );
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_check_with_module_import_style() {
        let source = r#"q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, Some("import sqlty"), false);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_untyped_query() {
        let source = r#"q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")"#;
        let diags = check_source(source, "postgres", &None, None, None, false);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "untyped-query");
        assert!(diags[0].message.contains("add casts"));
    }

    #[test]
    fn test_untyped_query_now_inferable() {
        let source = r#"q = sqlty.SQL[sqlty.UNTYPED]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, Some("import sqlty"), false);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "sqlty.SQL[sqlty.UNTYPED] can now be sqlty.SQL[tuple[int]]; run sqlty check --fix"
        );
    }
}
