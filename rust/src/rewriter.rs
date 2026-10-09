//! Python source rewriter — rewrite/strip `--sql` strings.
//!
//! Handles:
//! - `rewrite_source()`: `"--sql\n..." → SQL[T]("--sql\n...")`
//! - `strip_source()`: `SQL[T]("--sql\n...") → "--sql\n..."`
//!
//! The rewriter never modifies string content — it only adds/removes
//! the `SQL[T]()` wrapper.  This guarantees `strip(rewrite(x)) == x`.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{infer_type, returns_rows, Schema};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Import style parsing
// ---------------------------------------------------------------------------

/// Parsed import-style configuration.
struct ImportStyleParsed {
    /// The import statement to inject (e.g. "from sqlty import SQL as Q").
    import_line: String,
    /// The prefix for `[T](...)`, e.g. "Q" or "sqlty.SQL".
    wrapper_name: String,
}

impl ImportStyleParsed {
    /// How `UNTYPED` is spelled: "UNTYPED" or "sqlty.UNTYPED".
    fn untyped_name(&self) -> String {
        match self.wrapper_name.rsplit_once('.') {
            Some((module, _)) => format!("{}.UNTYPED", module),
            None => "UNTYPED".to_string(),
        }
    }

    /// Import line `UNTYPED` needs, or `None` when the module is imported.
    fn untyped_import(&self) -> Option<&'static str> {
        (!self.wrapper_name.contains('.')).then_some("from sqlty import UNTYPED")
    }
}

/// Parse an import-style string into its components.
///
/// Returns `None` for unrecognised styles.
fn parse_import_style(style: Option<&str>) -> ImportStyleParsed {
    match style {
        None | Some("from sqlty import SQL") => ImportStyleParsed {
            import_line: "from sqlty import SQL".to_string(),
            wrapper_name: "SQL".to_string(),
        },
        Some(s) if s.starts_with("from sqlty import SQL as ") => {
            let alias = s["from sqlty import SQL as ".len()..].trim();
            ImportStyleParsed {
                import_line: s.to_string(),
                wrapper_name: alias.to_string(),
            }
        }
        Some("import sqlty") => ImportStyleParsed {
            import_line: "import sqlty".to_string(),
            wrapper_name: "sqlty.SQL".to_string(),
        },
        Some(s) if s.starts_with("import sqlty as ") => {
            let alias = s["import sqlty as ".len()..].trim();
            ImportStyleParsed {
                import_line: s.to_string(),
                wrapper_name: format!("{}.SQL", alias),
            }
        }
        Some(_) => ImportStyleParsed {
            import_line: "from sqlty import SQL".to_string(),
            wrapper_name: "SQL".to_string(),
        },
    }
}

/// Build a regex that matches `{wrapper_name}[T]("...")` wrappers.
fn build_wrapper_pattern(wrapper_name: &str) -> Regex {
    let escaped = regex::escape(wrapper_name);
    Regex::new(&format!(
        r#"{}(?:\[.*?\]\(\s*(?:""".*?"""|'''.*?'''|"[^"]*"|'[^']*')\s*\))"#,
        escaped
    ))
    .unwrap()
}

/// Build a regex that extracts the inner string from `{wrapper_name}[T]("...")`.
fn build_wrapper_extract_pattern(wrapper_name: &str) -> Regex {
    let escaped = regex::escape(wrapper_name);
    Regex::new(&format!(
        r#"{}\[.*?\]\(\s*(""".*?"""|'''.*?'''|"[^"]*"|'[^']*')\s*\)"#,
        escaped
    ))
    .unwrap()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Rewrite Python source: `"--sql\n..."` → `SQL[T]("--sql\n...")`.
///
/// Adds necessary imports (SQL, datetime, uuid) at the top.
/// Never modifies string content — only adds the `SQL[T]()` wrapper.
/// With `untyped`, row-returning queries of unknown row type become `SQL[UNTYPED](...)`.
pub fn rewrite_source(
    source: &str,
    dialect: &str,
    schema: &Option<Schema>,
    import_style: Option<&str>,
    untyped: bool,
) -> String {
    let istyle = parse_import_style(import_style);
    let wrapper_pattern = build_wrapper_pattern(&istyle.wrapper_name);
    let untyped_name = untyped.then(|| istyle.untyped_name());

    let mut result_lines: Vec<String> = Vec::new();
    let mut any_rewritten = false;
    let mut any_untyped = false;

    for line in source.lines() {
        let (rewritten, is_untyped) = rewrite_line(
            line,
            dialect,
            schema,
            &istyle.wrapper_name,
            &wrapper_pattern,
            untyped_name.as_deref(),
        );
        if rewritten != line {
            any_rewritten = true;
            any_untyped |= is_untyped;
        }
        result_lines.push(rewritten);
    }

    let mut result = result_lines.join("\n");

    // Preserve trailing newline if original had one
    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    if any_rewritten {
        result = inject_imports(&result, &istyle, any_untyped);
    }

    result
}

/// Strip Python source: `SQL[T]("--sql\n...")` → `"--sql\n..."`.
///
/// Removes unused imports added by rewrite.
pub fn strip_source(source: &str, import_style: Option<&str>) -> String {
    let istyle = parse_import_style(import_style);
    let extract_pattern = build_wrapper_extract_pattern(&istyle.wrapper_name);

    let mut result_lines: Vec<String> = Vec::new();
    let mut any_stripped = false;

    for line in source.lines() {
        let stripped = strip_line(line, &extract_pattern);
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
        result = remove_unused_imports(&result, &istyle);
    }

    result
}

// ---------------------------------------------------------------------------
// Line-level processing
// ---------------------------------------------------------------------------

/// Rewrite a single line: detect `"--sql\n..."` and wrap with `{wrapper_name}[T](...)`.
///
/// Never modifies string content — only adds the wrapper. Returns the line
/// and whether it was wrapped as `untyped_name`.
fn rewrite_line(
    line: &str,
    dialect: &str,
    schema: &Option<Schema>,
    wrapper_name: &str,
    wrapper_pattern: &Regex,
    untyped_name: Option<&str>,
) -> (String, bool) {
    // Skip lines that already have wrapper
    if wrapper_pattern.is_match(line) {
        return (line.to_string(), false);
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
                    let untyped = untyped_name.filter(|_| inferred.is_none() && is_untyped(sql_content, dialect));
                    let is_untyped = untyped.is_some();

                    if let Some(type_str) = inferred.as_deref().or(untyped) {
                        let replacement = format!("{}[{}]({})", wrapper_name, type_str, string_literal);
                        return (line.replace(string_literal, &replacement), is_untyped);
                    }
                }
            }
        }
    }

    (line.to_string(), false)
}

/// Strip a single line: detect wrapper and unwrap.
fn strip_line(line: &str, extract_pattern: &Regex) -> String {
    if let Some(cap) = extract_pattern.captures(line) {
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

/// Whether `SQL[UNTYPED]` fits: rows are expected but their type is unknown.
///
/// Requires the `--sql` prefix to end its line; the space form is a SQL comment.
fn is_untyped(sql_content: &str, dialect: &str) -> bool {
    let rest = sql_content.strip_prefix("--sql").unwrap_or_default();
    let query = match rest.strip_prefix("\\n") {
        Some(query) => query,
        None if rest.starts_with('\n') || rest.starts_with("\r\n") => rest,
        None => return false,
    };
    returns_rows(query, dialect)
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

/// Inject necessary imports at the top of the source.
fn inject_imports(source: &str, istyle: &ImportStyleParsed, untyped: bool) -> String {
    let wrapper_marker = format!("{}[", istyle.wrapper_name);
    let needs_sql = source.contains(&wrapper_marker) && !has_import(source, &istyle.import_line);
    let needs_datetime = source.contains("datetime.") && !has_import(source, "import datetime");
    let needs_uuid = source.contains("uuid.") && !has_import(source, "import uuid");
    let untyped_import = istyle
        .untyped_import()
        .filter(|line| untyped && !has_import(source, line));

    if !needs_sql && !needs_datetime && !needs_uuid && untyped_import.is_none() {
        return source.to_string();
    }

    let mut imports = Vec::new();
    if needs_datetime {
        imports.push("import datetime".to_string());
    }
    if needs_uuid {
        imports.push("import uuid".to_string());
    }
    if needs_sql {
        imports.push(istyle.import_line.clone());
    }
    if let Some(line) = untyped_import {
        imports.push(line.to_string());
    }

    let import_block = imports.join("\n");
    format!("{}\n{}", import_block, source)
}

/// Remove unused imports from the source.
fn remove_unused_imports(source: &str, istyle: &ImportStyleParsed) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut result = Vec::new();
    let wrapper_marker = format!("{}[", istyle.wrapper_name);
    let has_sql_usage = source.contains(&wrapper_marker);
    let has_datetime_usage = source.contains("datetime.");
    let has_uuid_usage = source.contains("uuid.");
    let untyped_import = istyle.untyped_import();
    let has_untyped_usage = lines
        .iter()
        .any(|line| line.contains("UNTYPED") && Some(line.trim()) != untyped_import);

    for line in &lines {
        let trimmed = line.trim();

        if Some(trimmed) == untyped_import && !has_untyped_usage {
            continue;
        }

        // Remove configured import if wrapper no longer appears
        if trimmed == istyle.import_line && !has_sql_usage {
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
            r#"q = "--sql\nSELECT 1::INTEGER""#,
            "postgres",
            &None,
            None,
            false,
        );
        assert!(result.contains("SQL[tuple[int]]"));
        assert!(result.contains("from sqlty import SQL"));
    }

    #[test]
    fn test_rewrite_preserves_string_content() {
        // Rewriter never modifies string content — only adds SQL[T]() wrapper
        let result = rewrite_source(
            r#"q = "--sql\nSELECT 1::INTEGER""#,
            "postgres",
            &None,
            None,
            false,
        );
        assert!(result.contains(r#""--sql\nSELECT 1::INTEGER""#));
    }

    #[test]
    fn test_space_form_is_sql_comment_not_wrapped() {
        // --sql SELECT ... with space is a SQL comment — parser sees empty query
        let result = rewrite_source(
            r#"q = "--sql SELECT 1::INTEGER""#,
            "postgres",
            &None,
            None,
            false,
        );
        assert!(!result.contains("SQL["));
    }

    #[test]
    fn test_strip_basic() {
        let source = r#"from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")"#;
        let result = strip_source(source, None);
        assert!(!result.contains("SQL["));
        assert!(result.contains(r#""--sql\nSELECT 1::INTEGER""#));
    }

    #[test]
    fn test_rewrite_idempotent() {
        let source = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let once = rewrite_source(source, "postgres", &None, None, false);
        let twice = rewrite_source(&once, "postgres", &None, None, false);
        assert_eq!(once, twice);
    }

    #[test]
    fn test_strip_rewrite_roundtrip() {
        // strip(rewrite(source)) == source (ignoring imports)
        let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let rewritten = rewrite_source(original, "postgres", &None, None, false);
        let stripped = strip_source(&rewritten, None);
        assert_eq!(stripped.trim(), original.trim());
    }

    #[test]
    fn test_non_sql_unchanged() {
        let source = "x = 42\nname = \"hello\"";
        assert_eq!(rewrite_source(source, "postgres", &None, None, false), source);
    }

    #[test]
    fn test_strip_preserves_manual() {
        let source = r#"q = SQL[int]("SELECT 1")"#;
        assert_eq!(strip_source(source, None), source);
    }

    #[test]
    fn test_is_sqlty_managed() {
        assert!(is_sqlty_managed("--sql SELECT 1"));
        assert!(is_sqlty_managed("--sql\\nSELECT 1"));
        assert!(is_sqlty_managed("--sql\nSELECT 1"));
        assert!(!is_sqlty_managed("SELECT 1"));
        assert!(!is_sqlty_managed("custom query"));
    }

    #[test]
    fn test_rewrite_with_alias() {
        let result = rewrite_source(
            r#"q = "--sql\nSELECT 1::INTEGER""#,
            "postgres",
            &None,
            Some("from sqlty import SQL as Q"),
            false,
        );
        assert!(result.contains("Q[tuple[int]]"));
        assert!(result.contains("from sqlty import SQL as Q"));
        assert!(!result.contains("from sqlty import SQL\n"));
    }

    #[test]
    fn test_rewrite_with_module_import() {
        let result = rewrite_source(
            r#"q = "--sql\nSELECT 1::INTEGER""#,
            "postgres",
            &None,
            Some("import sqlty"),
            false,
        );
        assert!(result.contains("sqlty.SQL[tuple[int]]"));
        assert!(result.contains("import sqlty"));
    }

    #[test]
    fn test_rewrite_with_module_alias() {
        let result = rewrite_source(
            r#"q = "--sql\nSELECT 1::INTEGER""#,
            "postgres",
            &None,
            Some("import sqlty as sy"),
            false,
        );
        assert!(result.contains("sy.SQL[tuple[int]]"));
        assert!(result.contains("import sqlty as sy"));
    }

    #[test]
    fn test_strip_with_alias_roundtrip() {
        let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let style = Some("from sqlty import SQL as Q");
        let rewritten = rewrite_source(original, "postgres", &None, style, false);
        let stripped = strip_source(&rewritten, style);
        assert_eq!(stripped.trim(), original.trim());
    }

    #[test]
    fn test_strip_with_module_import_roundtrip() {
        let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let style = Some("import sqlty");
        let rewritten = rewrite_source(original, "postgres", &None, style, false);
        let stripped = strip_source(&rewritten, style);
        assert_eq!(stripped.trim(), original.trim());
    }

    #[test]
    fn test_strip_with_module_alias_roundtrip() {
        let original = r#"q = "--sql\nSELECT 1::INTEGER""#;
        let style = Some("import sqlty as sy");
        let rewritten = rewrite_source(original, "postgres", &None, style, false);
        let stripped = strip_source(&rewritten, style);
        assert_eq!(stripped.trim(), original.trim());
    }

    #[test]
    fn test_untyped_skipped_by_default() {
        let source = r#"q = "--sql\nVALUES (1, 2)""#;
        assert_eq!(rewrite_source(source, "postgres", &None, None, false), source);
    }

    #[test]
    fn test_untyped_sentinel() {
        let source = r#"q = "--sql\nVALUES (1, 2)""#;
        let result = rewrite_source(source, "postgres", &None, None, true);
        assert_eq!(
            result,
            "from sqlty import SQL\nfrom sqlty import UNTYPED\nq = SQL[UNTYPED](\"--sql\\nVALUES (1, 2)\")"
        );
        assert_eq!(strip_source(&result, None), source);
    }

    #[test]
    fn test_untyped_sentinel_module_import() {
        let source = r#"q = "--sql\nVALUES (1, 2)""#;
        let style = Some("import sqlty as sy");
        let result = rewrite_source(source, "postgres", &None, style, true);
        assert_eq!(
            result,
            "import sqlty as sy\nq = sy.SQL[sy.UNTYPED](\"--sql\\nVALUES (1, 2)\")"
        );
        assert_eq!(strip_source(&result, style), source);
    }

    #[test]
    fn test_untyped_skips_statements_without_rows() {
        for source in [
            r#"q = "--sql\nDELETE FROM users""#,
            r#"q = "--sql\nINSERT INTO users (id) VALUES (1)""#,
            r#"q = "--sql\nCREATE TABLE users (id INTEGER)""#,
            r#"q = "--sql\nBEGIN""#,
            r#"q = "--sql\n""#,
            r#"q = "--sql VALUES (1, 2)""#,
        ] {
            assert_eq!(rewrite_source(source, "postgres", &None, None, true), source);
        }
    }

    #[test]
    fn test_untyped_wraps_unparseable() {
        let source = r#"q = "--sql\nSELECT FROM users""#;
        let result = rewrite_source(source, "postgres", &None, None, true);
        assert!(result.ends_with(r#"q = SQL[UNTYPED]("--sql\nSELECT FROM users")"#));
    }
}
