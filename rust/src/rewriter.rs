//! Python source rewriter — rewrite/strip `--sql` strings.
//!
//! Handles:
//! - `rewrite_source()`: `"--sql ..." → SQL[T]("--sql\n...")`
//! - `strip_source()`: `SQL[T]("--sql\n...") → "--sql\n..."`

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{infer_type, Schema};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Pattern to detect SQL[T](...) wrappers.
static SQL_WRAPPER_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"SQL\[.*?\]\(\s*(?:""".*?"""|'''.*?'''|"[^"]*"|'[^']*')\s*\)"#).unwrap()
});

/// Pattern to match triple-quoted strings: """..."""
static TRIPLE_DOUBLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?s)""".*?""""#).unwrap());

/// Pattern to match triple-quoted strings: '''...'''
static TRIPLE_SINGLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)'''.*?'''").unwrap());

/// Pattern to match single-line double-quoted strings: "..."
static SINGLE_DOUBLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""[^"]*""#).unwrap());

/// Pattern to match single-line single-quoted strings: '...'
static SINGLE_SINGLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"'[^']*'").unwrap());

/// Pattern to extract inner string from SQL[T]("...")
static WRAPPER_EXTRACT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"SQL\[.*?\]\(\s*(""".*?"""|'''.*?'''|"[^"]*"|'[^']*')\s*\)"#).unwrap()
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Rewrite Python source: `"--sql ..."` → `SQL[T]("--sql\n...")`.
///
/// Adds necessary imports (SQL, datetime, uuid) at the top.
pub fn rewrite_source(source: &str, dialect: &str, schema: &Option<Schema>) -> String {
    let mut result_lines: Vec<String> = Vec::new();
    let mut any_rewritten = false;

    for line in source.lines() {
        let rewritten = rewrite_line(line, dialect, schema);
        if rewritten != line {
            any_rewritten = true;
        }
        result_lines.push(rewritten);
    }

    let mut result = result_lines.join("\n");

    // Preserve trailing newline if original had one
    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    if any_rewritten {
        result = inject_imports(&result);
    }

    result
}

/// Strip Python source: `SQL[T]("--sql\n...")` → `"--sql\n..."`.
///
/// Removes unused imports added by rewrite.
pub fn strip_source(source: &str) -> String {
    let mut result_lines: Vec<String> = Vec::new();
    let mut any_stripped = false;

    for line in source.lines() {
        let stripped = strip_line(line);
        if stripped != line {
            any_stripped = true;
        }
        result_lines.push(stripped);
    }

    let mut result = result_lines.join("\n");

    // Preserve trailing newline
    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    if any_stripped {
        result = remove_unused_imports(&result);
    }

    result
}

// ---------------------------------------------------------------------------
// Line-level processing
// ---------------------------------------------------------------------------

/// Rewrite a single line: detect `"--sql ..."` and wrap with `SQL[T](...)`.
fn rewrite_line(line: &str, dialect: &str, schema: &Option<Schema>) -> String {
    // Skip lines that already have SQL[...](...) wrappers
    if SQL_WRAPPER_PATTERN.is_match(line) {
        return line.to_string();
    }

    // Try each string pattern (triple-quoted first, then single-line)
    let patterns: &[&Lazy<Regex>] = &[
        &TRIPLE_DOUBLE_PATTERN,
        &TRIPLE_SINGLE_PATTERN,
        &SINGLE_DOUBLE_PATTERN,
        &SINGLE_SINGLE_PATTERN,
    ];

    for pattern in patterns {
        if let Some(m) = pattern.find(line) {
            let string_literal = m.as_str();
            let content = extract_string_content(string_literal);

            if let Some(content) = content {
                if content.trim_start().starts_with("--sql") {
                    let sql_content = content.trim_start();
                    let inferred = infer_from_sql_content(sql_content, dialect, schema);

                    if let Some(type_str) = inferred {
                        // Normalize: --sql<space> → --sql\n
                        let normalized = normalize_sql_prefix(string_literal);
                        let replacement = format!("SQL[{}]({})", type_str, normalized);
                        return line.replace(string_literal, &replacement);
                    }
                }
            }
        }
    }

    line.to_string()
}

/// Strip a single line: detect `SQL[T]("--sql ...")` and unwrap.
fn strip_line(line: &str) -> String {
    if let Some(cap) = WRAPPER_EXTRACT_PATTERN.captures(line) {
        let full_match = cap.get(0).unwrap().as_str();
        let inner = cap.get(1).unwrap().as_str();

        // Only strip if the inner content is sqlty-managed
        if let Some(content) = extract_string_content(inner) {
            if is_sqlty_managed(&content) {
                return line.replace(full_match, inner);
            }
        }
    }

    line.to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract string content from a quoted string literal (removes quotes).
fn extract_string_content(literal: &str) -> Option<String> {
    if literal.starts_with("\"\"\"") || literal.starts_with("'''") {
        Some(literal[3..literal.len() - 3].to_string())
    } else if literal.starts_with('"') || literal.starts_with('\'') {
        Some(literal[1..literal.len() - 1].to_string())
    } else {
        None
    }
}

/// Check if string content is sqlty-managed (starts with `--sql` + whitespace/\n).
fn is_sqlty_managed(content: &str) -> bool {
    let stripped = content.trim_start();
    if !stripped.starts_with("--sql") {
        return false;
    }
    let rest = &stripped[5..];
    if rest.is_empty() {
        return true;
    }
    // Source-text \n escape (backslash + n)
    if rest.starts_with("\\n") {
        return true;
    }
    // Whitespace: space, real newline, tab
    let first = rest.as_bytes()[0];
    matches!(first, b' ' | b'\n' | b'\r' | b'\t')
}

/// Normalize `--sql<space>` to `--sql\n` in a string literal.
fn normalize_sql_prefix(literal: &str) -> String {
    literal.replacen("--sql ", "--sql\\n", 1)
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

/// Inject necessary imports at the top of the source.
fn inject_imports(source: &str) -> String {
    let needs_sql = source.contains("SQL[") && !has_import(source, "from sqlty import SQL");
    let needs_datetime = source.contains("datetime.") && !has_import(source, "import datetime");
    let needs_uuid = source.contains("uuid.") && !has_import(source, "import uuid");

    if !needs_sql && !needs_datetime && !needs_uuid {
        return source.to_string();
    }

    let mut imports = Vec::new();
    if needs_datetime {
        imports.push("import datetime");
    }
    if needs_uuid {
        imports.push("import uuid");
    }
    if needs_sql {
        imports.push("from sqlty import SQL");
    }

    let import_block = imports.join("\n");
    format!("{}\n{}", import_block, source)
}

/// Remove unused imports from the source.
fn remove_unused_imports(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut result = Vec::new();
    let has_sql_usage = source.contains("SQL[");
    let has_datetime_usage = source.contains("datetime.");
    let has_uuid_usage = source.contains("uuid.");

    for line in &lines {
        let trimmed = line.trim();

        // Remove "from sqlty import SQL" if SQL[ no longer appears
        if trimmed == "from sqlty import SQL" && !has_sql_usage {
            continue;
        }
        // Remove "import datetime" if datetime. no longer appears
        if trimmed == "import datetime" && !has_datetime_usage {
            continue;
        }
        // Remove "import uuid" if uuid. no longer appears
        if trimmed == "import uuid" && !has_uuid_usage {
            continue;
        }

        result.push(*line);
    }

    let mut output = result.join("\n");
    if source.ends_with('\n') && !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

/// Check if source already has a specific import.
fn has_import(source: &str, import_str: &str) -> bool {
    source.lines().any(|line| line.trim() == import_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_basic() {
        let result = rewrite_source(
            r#"q = "--sql SELECT 1::INTEGER""#,
            "postgres",
            &None,
        );
        assert!(result.contains("SQL[tuple[int]]"));
        assert!(result.contains("from sqlty import SQL"));
    }

    #[test]
    fn test_rewrite_normalizes_newline() {
        let result = rewrite_source(
            r#"q = "--sql SELECT 1::INTEGER""#,
            "postgres",
            &None,
        );
        assert!(result.contains(r#""--sql\nSELECT 1::INTEGER""#));
    }

    #[test]
    fn test_strip_basic() {
        let source = r#"from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let result = strip_source(source);
        assert!(!result.contains("SQL["));
        assert!(result.contains(r#""--sql\nSELECT 1::INTEGER""#));
    }

    #[test]
    fn test_rewrite_idempotent() {
        let source = r#"q = "--sql SELECT 1::INTEGER""#;
        let once = rewrite_source(source, "postgres", &None);
        let twice = rewrite_source(&once, "postgres", &None);
        assert_eq!(once, twice);
    }

    #[test]
    fn test_non_sql_unchanged() {
        let source = "x = 42\nname = \"hello\"";
        assert_eq!(rewrite_source(source, "postgres", &None), source);
    }

    #[test]
    fn test_strip_preserves_manual() {
        let source = r#"q = SQL[int]("SELECT 1")"#;
        assert_eq!(strip_source(source), source);
    }

    #[test]
    fn test_is_sqlty_managed() {
        assert!(is_sqlty_managed("--sql SELECT 1"));
        assert!(is_sqlty_managed("--sql\\nSELECT 1"));
        assert!(is_sqlty_managed("--sql\nSELECT 1"));
        assert!(!is_sqlty_managed("SELECT 1"));
        assert!(!is_sqlty_managed("custom query"));
    }
}
