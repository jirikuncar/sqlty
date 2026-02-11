//! Lint checker for SQL type issues in Python source code.
//!
//! Checks for:
//! - `stale-rewrite`: `--sql` strings not wrapped in `SQL[T](...)`
//! - `stale-type`: `SQL[T]` annotation doesn't match current inference
//! - `select-star`: `SELECT *` usage
//! - `select-star-no-schema`: `SELECT *` with no schema available

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{infer_type, Schema};

/// Pattern to detect unwrapped --sql strings.
static UNWRAPPED_SQL_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"["']--sql(\s|\\n)"#).unwrap()
});

/// Pattern to extract SQL content from SQL[T]("--sql ...").
static SQL_CONTENT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"SQL\[.*?\]\(\s*["'](.+?)["']\s*\)"#).unwrap()
});

/// Pattern to extract type annotation from SQL[T](...).
static TYPE_ANNOTATION_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"SQL\[(.+?)\]\(").unwrap()
});

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
) -> Vec<Diagnostic> {
    let rule_config = RuleConfig::new(rules);
    let has_schema = schema.is_some();
    let mut diagnostics = Vec::new();

    for (i, line) in source.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        // Check for unwrapped --sql strings (stale-rewrite)
        if is_unwrapped_sql_string(trimmed) && rule_config.is_enabled("stale-rewrite") {
            diagnostics.push(Diagnostic {
                rule: "stale-rewrite".to_string(),
                message: "Source has --sql strings that aren't wrapped in SQL[T]".to_string(),
                severity: rule_config.severity("stale-rewrite").to_string(),
                line: line_num,
            });
            continue;
        }

        // Check for SQL[T](...) wrappers
        if let Some(sql_content) = extract_sql_from_wrapper(trimmed) {
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
                if let Some(annotated_type) = extract_type_from_wrapper(trimmed) {
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

/// Check if a line contains an unwrapped --sql string (not in SQL[T](...)).
fn is_unwrapped_sql_string(line: &str) -> bool {
    if line.contains("SQL[") {
        return false;
    }
    UNWRAPPED_SQL_PATTERN.is_match(line)
}

/// Extract SQL content from `SQL[T]("--sql ...")` pattern.
fn extract_sql_from_wrapper(line: &str) -> Option<String> {
    let cap = SQL_CONTENT_PATTERN.captures(line)?;
    let content = cap.get(1)?.as_str();
    if content.starts_with("--sql") {
        Some(content.to_string())
    } else {
        None
    }
}

/// Extract the type annotation from `SQL[T](...)` pattern.
fn extract_type_from_wrapper(line: &str) -> Option<String> {
    let cap = TYPE_ANNOTATION_PATTERN.captures(line)?;
    Some(cap.get(1)?.as_str().to_string())
}

/// Infer type from SQL content (after removing `--sql` prefix).
fn infer_from_sql_content(sql_content: &str, dialect: &str, schema: &Option<Schema>) -> Option<String> {
    let query = if sql_content.starts_with("--sql ") {
        sql_content[6..].trim()
    } else if sql_content.starts_with("--sql\\n") {
        sql_content[7..].trim()
    } else if sql_content.starts_with("--sql\n") {
        sql_content[6..].trim()
    } else if sql_content.starts_with("--sql") {
        sql_content[5..].trim()
    } else {
        return None;
    };

    infer_type(query, dialect, schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_stale_rewrite() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let diags = check_source(source, "postgres", &None, None);
        assert!(!diags.is_empty());
        assert_eq!(diags[0].rule, "stale-rewrite");
    }

    #[test]
    fn test_clean_no_stale() {
        let source = r#"q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let diags = check_source(source, "postgres", &None, None);
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }

    #[test]
    fn test_detects_stale_type() {
        let source = r#"q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")"#;
        let diags = check_source(source, "postgres", &None, None);
        let stale_types: Vec<_> = diags.iter().filter(|d| d.rule == "stale-type").collect();
        assert_eq!(stale_types.len(), 1);
        assert!(stale_types[0].message.contains("tuple[int]"));
    }

    #[test]
    fn test_rule_override_ignore() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let mut rules = HashMap::new();
        rules.insert("stale-rewrite".to_string(), "ignore".to_string());
        let diags = check_source(source, "postgres", &None, Some(rules));
        let stale_rewrites: Vec<_> = diags.iter().filter(|d| d.rule == "stale-rewrite").collect();
        assert!(stale_rewrites.is_empty());
    }
}
