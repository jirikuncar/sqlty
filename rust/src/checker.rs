//! Lint checker for SQL type issues in Python source code.
//!
//! Checks for:
//! - `stale-rewrite`: `--sql` strings not wrapped in `SQL[T](...)`
//! - `stale-type`: `SQL[T]` annotation doesn't match current inference
//! - `select-star`: `SELECT *` usage
//! - `select-star-no-schema`: `SELECT *` with no schema available
//! - `inline-sql-comment`: `"--sql SELECT ..."` (space) makes query a SQL comment

use std::collections::HashMap;

use regex::Regex;

use crate::analyzer::{infer_type, Schema};

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
    Regex::new(&format!(
        r#"{}\[.*?\]\(\s*["'](.+?)["']\s*\)"#,
        escaped
    ))
    .unwrap()
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
            "inline-sql-comment" => "warn",
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
) -> Vec<Diagnostic> {
    let rule_config = RuleConfig::new(rules);
    let has_schema = schema.is_some();
    let mut diagnostics = Vec::new();

    let wrapper_name = parse_wrapper_name(import_style);
    let wrapper_marker = format!("{}[", wrapper_name);
    let sql_content_pat = build_sql_content_pattern(&wrapper_name);
    let type_annotation_pat = build_type_annotation_pattern(&wrapper_name);
    let unwrapped_pat = unwrapped_sql_pattern();
    let inline_pat = inline_sql_comment_pattern();

    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        // Check for unwrapped --sql strings (stale-rewrite)
        if is_unwrapped_sql_string(trimmed, &wrapper_marker, &unwrapped_pat) {
            if rule_config.is_enabled("stale-rewrite") {
                diagnostics.push(Diagnostic {
                    rule: "stale-rewrite".to_string(),
                    message: "Source has --sql strings that aren't wrapped in SQL[T]".to_string(),
                    severity: rule_config.severity("stale-rewrite").to_string(),
                    line: line_num,
                });
            }
            // Also check for inline-sql-comment on unwrapped strings
            if rule_config.is_enabled("inline-sql-comment") && has_inline_sql_comment(trimmed, &inline_pat) {
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
            if sql_content.contains("SELECT *") || sql_content.contains("select *") {
                if has_schema && rule_config.is_enabled("select-star") {
                    diagnostics.push(Diagnostic {
                        rule: "select-star".to_string(),
                        message: "SELECT * used — consider explicit column list".to_string(),
                        severity: rule_config.severity("select-star").to_string(),
                        line: line_num,
                    });
                } else if !has_schema && rule_config.is_enabled("select-star-no-schema") {
                    diagnostics.push(Diagnostic {
                        rule: "select-star-no-schema".to_string(),
                        message: "SELECT * used but no schema available for expansion".to_string(),
                        severity: rule_config.severity("select-star-no-schema").to_string(),
                        line: line_num,
                    });
                }
            }

            // Check stale types
            if rule_config.is_enabled("stale-type") {
                if let Some(annotated_type) = extract_type_from_wrapper(trimmed, &type_annotation_pat) {
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

/// Infer type from SQL content by letting the SQL parser handle `--sql` as a comment.
///
/// - `--sql\nSELECT ...` → `\\n` becomes real newline; parser sees `--sql`
///   as a comment and `SELECT ...` as the query.
/// - `--sql SELECT ...` → entire line is a SQL comment → empty query → None.
fn infer_from_sql_content(sql_content: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let mut query = sql_content.to_string();

    // Convert source-text \\n escape to real newline so the parser
    // sees --sql as a comment and the query on the next line.
    if query.starts_with("--sql\\n") {
        query = query.replacen("\\n", "\n", 1);
    }

    infer_type(&query, dialect, schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_stale_rewrite() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None, None);
        assert!(!diags.is_empty());
        assert_eq!(diags[0].rule, "stale-rewrite");
    }

    #[test]
    fn test_clean_no_stale() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_stale_type() {
        let source = r#"q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")"#;
        let diags = check_source(source, "postgres", &None, None, None);
        let stale_types: Vec<_> = diags.iter().filter(|d| d.rule == "stale-type").collect();
        assert_eq!(stale_types.len(), 1);
        assert!(stale_types[0].message.contains("tuple[int]"));
    }

    #[test]
    fn test_rule_override_ignore() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let mut rules = HashMap::new();
        rules.insert("stale-rewrite".to_string(), "ignore".to_string());
        let diags = check_source(source, "postgres", &None, Some(rules), None);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_unwrapped() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None, None);
        let inline: Vec<_> = diags.iter().filter(|d| d.rule == "inline-sql-comment").collect();
        assert_eq!(inline.len(), 1);
        assert!(inline[0].message.contains("SQL comment"));
    }

    #[test]
    fn test_no_inline_sql_comment_with_newline() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None);
        let inline: Vec<_> = diags.iter().filter(|d| d.rule == "inline-sql-comment").collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_detects_inline_sql_comment_wrapped() {
        let source = r#"q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, None);
        let inline: Vec<_> = diags.iter().filter(|d| d.rule == "inline-sql-comment").collect();
        assert_eq!(inline.len(), 1);
    }

    #[test]
    fn test_inline_sql_comment_ignored_when_disabled() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let mut rules = HashMap::new();
        rules.insert("inline-sql-comment".to_string(), "ignore".to_string());
        let diags = check_source(source, "postgres", &None, Some(rules), None);
        let inline: Vec<_> = diags.iter().filter(|d| d.rule == "inline-sql-comment").collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn test_check_with_alias_import_style() {
        let source = r#"q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, Some("from sqlty import SQL as Q"));
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_check_with_module_import_style() {
        let source = r#"q = sqlty.SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None, Some("import sqlty"));
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }
}
