//! Python source rewriter — rewrite/strip `--sql` strings.
//!
//! Handles:
//! - `rewrite_source()`: `"--sql\n..." → SQL[T]("--sql\n...")`
//! - `strip_source()`: `SQL[T]("--sql\n...") → "--sql\n..."`
//!
//! Both work on the whole source, so triple-quoted strings spanning lines are
//! handled, and mirror `sqlty._rewriter.SQLRewriter` match for match.
//! The rewriter never modifies string content — it only adds/removes
//! the `SQL[T]()` wrapper.  This guarantees `strip(rewrite(x)) == x`.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{infer_type, returns_rows, Schema};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// A string literal starting at the beginning of the haystack; triple quotes first.
static STRING_LITERAL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^(?s:""".*?"""|'''.*?'''|"[^"]*"|'[^']*')"#).unwrap());

static CODING: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#.*?coding[:=]").unwrap());

/// f-strings and t-strings (PEP 750) with a `--sql` prefix: interpolated, so never rewritten.
static FSTRING_SQL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"[fFtT][rRbBuU]*(?:"{3}|'{3}|"|')--sql\b|[rRbBuU]*[fFtT](?:"{3}|'{3}|"|')--sql\b"#)
        .unwrap()
});

/// `fstring-skipped` or `tstring-skipped` when `line` has an interpolated `--sql` string.
pub(crate) fn skipped_string_rule(line: &str) -> Option<&'static str> {
    let prefix = FSTRING_SQL.find(line)?.as_str();
    Some(if prefix.contains(['t', 'T']) {
        "tstring-skipped"
    } else {
        "fstring-skipped"
    })
}

/// Spans of string literals not prefixed by a letter (`f"..."`, `r"..."` are skipped).
///
/// Equivalent to `re.finditer` with the lookbehind `(?<![a-zA-Z])`, which the
/// `regex` crate lacks.
fn string_literals(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut spans = Vec::new();
    let mut pos = 0;
    while let Some(offset) = source[pos..].find(['"', '\'']) {
        let start = pos + offset;
        let literal = (start == 0 || !bytes[start - 1].is_ascii_alphabetic())
            .then(|| STRING_LITERAL.find(&source[start..]))
            .flatten();
        pos = match literal {
            Some(m) => {
                spans.push((start, start + m.end()));
                start + m.end()
            }
            None => start + 1,
        };
    }
    spans
}

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

    /// Regex matching `{wrapper_name}[T]("...")`; group `inner` is the string literal.
    ///
    /// `T` stays on one line; the string may sit on its own lines with a trailing
    /// comma, as formatters lay out long calls.
    fn wrapper_pattern(&self) -> Regex {
        Regex::new(&format!(
            r#"(?s){}\[[^\n]*?\]\(\s*(?P<inner>""".*?"""|'''.*?'''|"[^"]*"|'[^']*')\s*,?\s*\)"#,
            regex::escape(&self.wrapper_name)
        ))
        .unwrap()
    }
}

/// Parse an import-style string into its components.
///
/// Unrecognised styles fall back to `from sqlty import SQL`.
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
    let fstring_lines: Vec<usize> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| FSTRING_SQL.is_match(line))
        .map(|(i, _)| i + 1)
        .collect();
    let wrapper_spans: Vec<(usize, usize)> = istyle
        .wrapper_pattern()
        .find_iter(source)
        .map(|m| (m.start(), m.end()))
        .collect();

    let mut replacements = Vec::new();
    let mut needs_untyped_import = false;
    for (start, end) in string_literals(source) {
        if wrapper_spans
            .iter()
            .any(|&(ws, we)| ws <= start && end <= we)
            || (!fstring_lines.is_empty() && fstring_lines.contains(&line_at(source, start)))
        {
            continue;
        }
        let literal = &source[start..end];
        let content = extract_string_content(literal).trim();
        if !content.starts_with("--sql") {
            continue;
        }
        let type_str = match infer_sql_content(content, dialect, schema) {
            Some(inferred) => inferred,
            None if untyped && is_untyped(content, dialect) => {
                needs_untyped_import = true;
                istyle.untyped_name()
            }
            // Don't wrap when inference fails — SQL[Any] is never useful.
            None => continue,
        };
        let replacement = format!("{}[{}]({})", istyle.wrapper_name, type_str, literal);
        replacements.push((start, end, replacement));
    }

    if replacements.is_empty() {
        return source.to_string();
    }
    let needs = |marker: &str| replacements.iter().any(|(_, _, r)| r.contains(marker));
    let mut imports = Vec::new();
    if needs("datetime.") && !has_import(source, "import datetime") {
        imports.push("import datetime");
    }
    if needs("uuid.") && !has_import(source, "import uuid") {
        imports.push("import uuid");
    }
    if !has_import(source, &istyle.import_line) {
        imports.push(&istyle.import_line);
    }
    if let Some(line) = istyle.untyped_import() {
        if needs_untyped_import && !has_import(source, line) {
            imports.push(line);
        }
    }

    let result = splice(source, &replacements);
    if imports.is_empty() {
        return result;
    }
    let at = header_end(&result);
    let sep = if at > 0 && !result[..at].ends_with('\n') {
        "\n"
    } else {
        ""
    };
    format!(
        "{}{}{}\n{}",
        &result[..at],
        sep,
        imports.join("\n"),
        &result[at..]
    )
}

/// Strip Python source: `SQL[T]("--sql\n...")` → `"--sql\n..."`.
///
/// Removes unused imports added by rewrite.
pub fn strip_source(source: &str, import_style: Option<&str>) -> String {
    let istyle = parse_import_style(import_style);
    let replacements: Vec<(usize, usize, String)> = istyle
        .wrapper_pattern()
        .captures_iter(source)
        .filter_map(|cap| {
            let wrapper = cap.get(0)?;
            let inner = cap.name("inner")?.as_str();
            is_sqlty_managed(extract_string_content(inner))
                .then(|| (wrapper.start(), wrapper.end(), inner.to_string()))
        })
        .collect();

    if replacements.is_empty() {
        return source.to_string();
    }
    remove_unused_imports(&splice(source, &replacements), &istyle)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply non-overlapping `(start, end, replacement)` edits, given in source order.
fn splice(source: &str, replacements: &[(usize, usize, String)]) -> String {
    let mut result = String::with_capacity(source.len());
    let mut pos = 0;
    for (start, end, replacement) in replacements {
        result.push_str(&source[pos..*start]);
        result.push_str(replacement);
        pos = *end;
    }
    result.push_str(&source[pos..]);
    result
}

/// 1-indexed line number of a byte position.
fn line_at(source: &str, pos: usize) -> usize {
    source[..pos].matches('\n').count() + 1
}

/// Extract string content from a quoted string literal (removes quotes).
fn extract_string_content(literal: &str) -> &str {
    let quote = if literal.starts_with("\"\"\"") || literal.starts_with("'''") {
        3
    } else {
        1
    };
    &literal[quote..literal.len() - quote]
}

/// Check if string content is sqlty-managed (starts with `--sql` + whitespace/\n).
fn is_sqlty_managed(content: &str) -> bool {
    let Some(rest) = content.trim_start().strip_prefix("--sql") else {
        return false;
    };
    rest.is_empty() || rest.starts_with("\\n") || rest.starts_with([' ', '\n', '\r', '\t'])
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

/// Infer the row type of `--sql` string content.
///
/// - `--sql\nSELECT ...` → `\\n` becomes a real newline, and
///   `analyzer::parser_input` drops the marker line before parsing.
/// - `--sql SELECT ...` → there is no newline, so nothing is left to parse → None.
///
/// Inference errors (an unknown `SUM` column) count as uninferable.
pub(crate) fn infer_sql_content(
    sql_content: &str,
    dialect: &str,
    schema: &Option<Schema>,
) -> Option<String> {
    let query = match sql_content.strip_prefix("--sql\\n") {
        Some(rest) => format!("--sql\n{rest}"),
        None => sql_content.to_string(),
    };
    infer_type(&query, dialect, schema).ok().flatten()
}

/// Remove sqlty/datetime/uuid imports that are no longer referenced.
fn remove_unused_imports(source: &str, istyle: &ImportStyleParsed) -> String {
    let wrapper_marker = format!("{}[", istyle.wrapper_name);
    let untyped_import = istyle.untyped_import();
    let untyped_used = source
        .lines()
        .any(|line| line.contains("UNTYPED") && Some(line.trim()) != untyped_import);

    source
        .split_inclusive('\n')
        .filter(|line| {
            let line = line.trim_end();
            !((Some(line) == untyped_import && !untyped_used)
                || (line == istyle.import_line && !source.contains(&wrapper_marker))
                || (line == "import datetime" && !source.contains("datetime."))
                || (line == "import uuid" && !source.contains("uuid.")))
        })
        .collect()
}

/// Check for a top-level import; indented lines (docstrings, blocks) don't count.
fn has_import(source: &str, import_str: &str) -> bool {
    source.lines().any(|line| line.trim_end() == import_str)
}

/// Offset where imports go: after shebang, coding cookie, docstring and `__future__`.
pub(crate) fn header_end(source: &str) -> usize {
    let line_end_at = |from: usize| {
        source[from..]
            .find('\n')
            .map_or(source.len(), |i| from + i + 1)
    };
    let (mut pos, mut end, mut lineno) = (0, 0, 0);
    let mut docstring = true;
    while pos < source.len() {
        let mut line_end = line_end_at(pos);
        let line = source[pos..line_end].trim();
        lineno += 1;
        if line.is_empty() || line.starts_with('#') {
            if (pos == 0 && line.starts_with("#!")) || (lineno <= 2 && CODING.is_match(line)) {
                end = line_end;
            }
            pos = line_end;
            continue;
        }
        if docstring {
            if let Some(close) = docstring_end(source, pos) {
                docstring = false;
                pos = close;
                end = close;
                continue;
            }
        }
        if !line.starts_with("from __future__ import") {
            break;
        }
        docstring = false;
        if line.contains('(') && !line.contains(')') {
            line_end = source[pos..]
                .find(')')
                .map_or(source.len(), |i| line_end_at(pos + i));
        }
        pos = line_end;
        end = line_end;
    }
    end
}

/// End of the line closing a string-literal statement starting at `pos`.
fn docstring_end(source: &str, pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = pos;
    if matches!(bytes.get(i), Some(b'r' | b'R' | b'u' | b'U')) {
        i += 1;
    }
    let quote = ["\"\"\"", "'''", "\"", "'"]
        .into_iter()
        .find(|q| source[i..].starts_with(q))?;
    i += quote.len();
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            _ if bytes[i..].starts_with(quote.as_bytes()) => break,
            b'\n' if quote.len() == 1 => return None,
            _ => i += 1,
        }
    }
    if i >= bytes.len() {
        return None;
    }
    let rest_end = source[i..].find('\n').map_or(source.len(), |n| i + n);
    let rest = source[i + quote.len()..rest_end].trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return None;
    }
    Some((rest_end + 1).min(source.len()))
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
        assert_eq!(
            rewrite_source(source, "postgres", &None, None, false),
            source
        );
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
        assert_eq!(
            rewrite_source(source, "postgres", &None, None, false),
            source
        );
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
            assert_eq!(
                rewrite_source(source, "postgres", &None, None, true),
                source
            );
        }
    }

    #[test]
    fn test_untyped_wraps_unparseable() {
        let source = r#"q = "--sql\nSELECT FROM users""#;
        let result = rewrite_source(source, "postgres", &None, None, true);
        assert!(result.ends_with(r#"q = SQL[UNTYPED]("--sql\nSELECT FROM users")"#));
    }

    #[test]
    fn test_multiline_triple_quoted_roundtrip() {
        let source = "q = \"\"\"--sql\nSELECT id::INTEGER\nFROM users\"\"\"\n";
        let result = rewrite_source(source, "postgres", &None, None, false);
        assert_eq!(
            result,
            "from sqlty import SQL\nq = SQL[tuple[int]](\"\"\"--sql\nSELECT id::INTEGER\nFROM users\"\"\")\n"
        );
        assert_eq!(strip_source(&result, None), source);
    }

    #[test]
    fn test_prefixed_strings_skipped() {
        let source = "a = f\"--sql\\nSELECT {x}::INTEGER\"\nb = r\"--sql\\nSELECT 1::INTEGER\"";
        assert_eq!(
            rewrite_source(source, "postgres", &None, None, false),
            source
        );
    }
}
