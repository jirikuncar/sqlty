//! Python source rewriter — rewrite/strip `--sql` strings.
//!
//! Handles:
//! - `rewrite_source()`: `"--sql\n..." → SQL[T]("--sql\n...")`
//! - `strip_source()`: `SQL[T]("--sql\n...") → "--sql\n..."`
//!
//! Both work on the whole source, so triple-quoted strings spanning lines are
//! handled.
//! The rewriter never modifies string content — it only adds/removes
//! the `SQL[T]()` wrapper.  This guarantees `strip(rewrite(x)) == x`.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{infer_type, returns_rows, Schema};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// A string literal starting at the beginning of the haystack; triple quotes first,
/// backslash escapes skipped.
static STRING_LITERAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"^(?s:"""(?:\\.|[^\\])*?"""|'''(?:\\.|[^\\])*?'''|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')"#,
    )
    .unwrap()
});

/// A valid string prefix (`rb`, `f`, `u`, ...), possibly empty.
static STRING_PREFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i:[bft]r?|r[bft]?|u)?$").unwrap());

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

/// Spans of plain string literals (no prefix but `u`), skipping comments.
///
/// Raw, bytes, f- and t-strings are scanned past, so their closing quote is not
/// paired with the next opening one. A quote after letters that are no string
/// prefix (`it's`) opens no literal. Mirrors `_SQL_STRING_PATTERN` in Python.
pub(crate) fn string_literals(source: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut pos = 0;
    while let Some(offset) = source[pos..].find(['"', '\'', '#']) {
        let start = pos + offset;
        if source[start..].starts_with('#') {
            pos = source[start..]
                .find('\n')
                .map_or(source.len(), |n| start + n);
            continue;
        }
        let before = &source[..start];
        let prefix = &before[before
            .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
            .len()..];
        pos = match STRING_LITERAL.find(&source[start..]) {
            Some(m) if STRING_PREFIX.is_match(prefix) => {
                if prefix.is_empty() || prefix.eq_ignore_ascii_case("u") {
                    spans.push((start - prefix.len(), start + m.end()));
                }
                start + m.end()
            }
            _ => start + 1,
        };
    }
    spans
}

// ---------------------------------------------------------------------------
// Import style parsing
// ---------------------------------------------------------------------------

/// Parsed `import-style`: the import line to inject and the `[T](...)` wrapper prefix.
///
/// `from sqlty import SQL` -> `SQL`, `from sqlty import SQL as Q` -> `Q`,
/// `import sqlty` -> `sqlty.SQL`, `import sqlty as sy` -> `sy.SQL`.
pub(crate) struct ImportStyle {
    pub import_line: String,
    pub wrapper_name: String,
}

impl Default for ImportStyle {
    fn default() -> Self {
        ImportStyle {
            import_line: "from sqlty import SQL".to_string(),
            wrapper_name: "SQL".to_string(),
        }
    }
}

impl ImportStyle {
    pub fn parse(style: Option<&str>) -> Result<Self, String> {
        let Some(style) = style.map(str::trim).filter(|style| !style.is_empty()) else {
            return Ok(Self::default());
        };
        let wrapper_name = if style == "from sqlty import SQL" {
            "SQL".to_string()
        } else if let Some(alias) = style.strip_prefix("from sqlty import SQL as ") {
            alias.trim().to_string()
        } else if style == "import sqlty" {
            "sqlty.SQL".to_string()
        } else if let Some(alias) = style.strip_prefix("import sqlty as ") {
            format!("{}.SQL", alias.trim())
        } else {
            return Err(format!("Unsupported import-style: '{style}'"));
        };
        Ok(ImportStyle {
            import_line: style.to_string(),
            wrapper_name,
        })
    }

    /// [`ImportStyle::parse`], with the default for an unsupported style.
    pub fn or_default(style: Option<&str>) -> Self {
        Self::parse(style).unwrap_or_default()
    }

    /// How `UNTYPED` is spelled: "UNTYPED" or "sqlty.UNTYPED".
    pub fn untyped_name(&self) -> String {
        match self.wrapper_name.rsplit_once('.') {
            Some((module, _)) => format!("{}.UNTYPED", module),
            None => "UNTYPED".to_string(),
        }
    }

    /// Import line `UNTYPED` needs, or `None` when the module is imported.
    pub fn untyped_import(&self) -> Option<&'static str> {
        (!self.wrapper_name.contains('.')).then_some("from sqlty import UNTYPED")
    }

    /// Regex matching `{wrapper_name}[T]("...")`; group `inner` is the string literal.
    ///
    /// `T` stays on one line; the string may sit on its own lines with a trailing
    /// comma, as formatters lay out long calls.
    fn wrapper_pattern(&self) -> Regex {
        Regex::new(&format!(
            r#"(?s){}\[[^\n]*?\]\(\s*(?P<inner>[uU]?(?:""".*?"""|'''.*?'''|"[^"]*"|'[^']*'))\s*,?\s*\)"#,
            regex::escape(&self.wrapper_name)
        ))
        .unwrap()
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
    rewrite_source_with(source, dialect, schema, import_style, untyped, false)
}

/// [`rewrite_source`]; with `keep_lines`, imports go on existing lines (see [`insert_inline`]).
pub fn rewrite_source_with(
    source: &str,
    dialect: &str,
    schema: &Option<Schema>,
    import_style: Option<&str>,
    untyped: bool,
    keep_lines: bool,
) -> String {
    let istyle = ImportStyle::or_default(import_style);
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
        if after_marker(content).is_none() {
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
    if keep_lines {
        return insert_inline(&result, &imports.join("; "));
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
    let istyle = ImportStyle::or_default(import_style);
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
pub(crate) fn splice(source: &str, replacements: &[(usize, usize, String)]) -> String {
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
pub(crate) fn line_at(source: &str, pos: usize) -> usize {
    source[..pos].matches('\n').count() + 1
}

/// Extract string content from a quoted string literal (removes quotes and a `u` prefix).
pub(crate) fn extract_string_content(literal: &str) -> &str {
    let literal = literal.trim_start_matches(['u', 'U']);
    let quote = if literal.starts_with("\"\"\"") || literal.starts_with("'''") {
        3
    } else {
        1
    };
    &literal[quote..literal.len() - quote]
}

/// Line breaks that can end the marker line: escaped in source text, or real.
const LINE_BREAKS: [&str; 4] = ["\\r\\n", "\\n", "\r\n", "\n"];

/// String content after the sqlty marker: `--sql`, or `-- sql` ending its line
/// (see [`ends_marker_line`]), a form MySQL also reads as a comment.
pub(crate) fn after_marker(content: &str) -> Option<&str> {
    let content = content.trim_start();
    if let Some(rest) = content.strip_prefix("--sql") {
        return Some(rest);
    }
    let rest = content.strip_prefix("-- sql")?;
    ends_marker_line(rest).then_some(rest)
}

/// Whether the rest of a marker line is blank or a `--` comment, up to a line break.
pub(crate) fn ends_marker_line(rest: &str) -> bool {
    let Some(end) = LINE_BREAKS.iter().filter_map(|lb| rest.find(lb)).min() else {
        return false;
    };
    let line = &rest[..end];
    let text = line.trim_start_matches([' ', '\t']);
    text.trim_end().is_empty() || (text.len() < line.len() && text.starts_with("--"))
}

pub(crate) fn starts_with_line_break(text: &str) -> bool {
    LINE_BREAKS.iter().any(|lb| text.starts_with(lb))
}

/// `text` without one leading line break, escaped or real.
pub(crate) fn strip_line_break(text: &str) -> Option<&str> {
    LINE_BREAKS.iter().find_map(|lb| text.strip_prefix(lb))
}

/// Check if string content is sqlty-managed: the marker, then whitespace or a line break.
pub(crate) fn is_sqlty_managed(content: &str) -> bool {
    let Some(rest) = after_marker(content) else {
        return false;
    };
    rest.is_empty() || starts_with_line_break(rest) || rest.starts_with([' ', '\t'])
}

/// Whether `SQL[UNTYPED]` fits: rows are expected but their type is unknown.
///
/// Requires the marker to end its line; the space form is a SQL comment.
fn is_untyped(sql_content: &str, dialect: &str) -> bool {
    after_marker(sql_content)
        .and_then(strip_line_break)
        .is_some_and(|query| returns_rows(query, dialect))
}

/// Infer the row type of marked string content.
///
/// - `--sql\nSELECT ...` → an escaped line break (`\\n`, `\\r\\n`) becomes a real
///   one, and `analyzer::parser_input` drops the marker line before parsing.
/// - `--sql SELECT ...` → there is no line break, so nothing is left to parse → None.
///
/// Inference errors (an unknown `SUM` column) count as uninferable.
pub(crate) fn infer_sql_content(
    sql_content: &str,
    dialect: &str,
    schema: &Option<Schema>,
) -> Option<String> {
    let rest = after_marker(sql_content)?;
    let rest = match rest.strip_prefix("\\r\\n").or(rest.strip_prefix("\\n")) {
        Some(query) => format!("\n{query}"),
        None => rest.to_string(),
    };
    infer_type(&format!("--sql{rest}"), dialect, schema)
        .ok()
        .flatten()
}

/// Remove sqlty/datetime/uuid imports that are no longer referenced.
fn remove_unused_imports(source: &str, istyle: &ImportStyle) -> String {
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
pub(crate) fn has_import(source: &str, import_str: &str) -> bool {
    source.lines().any(|line| line.trim_end() == import_str)
}

/// Offset where imports go: after shebang, coding cookie, docstring and `__future__`.
pub(crate) fn header_end(source: &str) -> usize {
    header(source).0
}

/// [`header_end`], and where the code of the header's last statement ends, if it has one.
fn header(source: &str) -> (usize, Option<usize>) {
    let line_end_at = |from: usize| {
        source[from..]
            .find('\n')
            .map_or(source.len(), |i| from + i + 1)
    };
    let (mut pos, mut end, mut lineno) = (0, 0, 0);
    let mut stmt = None;
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
            if let Some((close, close_line_end)) = docstring_end(source, pos) {
                docstring = false;
                stmt = Some(close);
                pos = close_line_end;
                end = close_line_end;
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
        let last = source[..line_end].trim_end_matches(['\n', '\r']);
        let last_start = last.rfind('\n').map_or(0, |i| i + 1);
        let code = last[last_start..].split('#').next().unwrap_or_default();
        stmt = Some(last_start + code.trim_end().len());
        pos = line_end;
        end = line_end;
    }
    (end, stmt)
}

/// Add `imports` (`; `-joined) to `source` without adding a line, so line numbers stay the same.
///
/// CPython decodes a script from the end of its coding line and drops the first decoded line,
/// so imports never go on a line of their own above the first statement. They are appended to
/// the docstring or last `__future__` import, else put in front of the first statement, else
/// (a compound statement) on a blank or comment line above it after line 1. Only a compound
/// statement right after the shebang and coding lines gets a new line.
fn insert_inline(source: &str, imports: &str) -> String {
    let (mut pos, stmt) = header(source);
    if let Some(end) = stmt {
        return format!("{}; {}{}", &source[..end], imports, &source[end..]);
    }
    let first_line_end = source.find('\n').map_or(source.len(), |i| i + 1);
    let mut spare = None;
    while pos < source.len() {
        let line_end = source[pos..]
            .find('\n')
            .map_or(source.len(), |i| pos + i + 1);
        let line = source[pos..line_end].trim();
        if !line.is_empty() && !line.starts_with('#') {
            break;
        }
        if pos >= first_line_end && spare.is_none() {
            spare = Some((pos, if line.is_empty() { "" } else { "  " }));
        }
        pos = line_end;
    }
    let line = source[pos..].lines().next().unwrap_or_default();
    if let Some(close) =
        string_end(source, pos).filter(|&c| source[c..].trim_start().starts_with(';'))
    {
        return format!("{}; {}{}", &source[..close], imports, &source[close..]);
    }
    let (at, text) = match spare {
        _ if !is_compound(line) => (pos, format!("{imports}; ")),
        Some((at, sep)) => (at, format!("{imports}{sep}")),
        None => (pos, format!("{imports}\n")),
    };
    format!("{}{}{}", &source[..at], text, &source[at..])
}

/// Whether a statement starting with `line` can't follow `; `.
fn is_compound(line: &str) -> bool {
    let word = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default();
    line.starts_with('@')
        || matches!(
            word,
            "def" | "class" | "if" | "for" | "while" | "with" | "try" | "async"
        )
        || (word == "match"
            && line
                .split('#')
                .next()
                .unwrap_or_default()
                .trim_end()
                .ends_with(':'))
}

/// End of a string literal starting at `pos`, after its closing quote.
fn string_end(source: &str, pos: usize) -> Option<usize> {
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
            _ if bytes[i..].starts_with(quote.as_bytes()) => return Some(i + quote.len()),
            b'\n' if quote.len() == 1 => return None,
            _ => i += 1,
        }
    }
    None
}

/// End of a string-literal statement starting at `pos`, and of the line closing it.
fn docstring_end(source: &str, pos: usize) -> Option<(usize, usize)> {
    let close = string_end(source, pos)?;
    let rest_end = source[close..]
        .find('\n')
        .map_or(source.len(), |n| close + n);
    let rest = source[close..rest_end].trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return None;
    }
    Some((close, (rest_end + 1).min(source.len())))
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
    fn test_string_literals_skip_past_prefixed_strings() {
        let source = "# it's\nf = f\"a\" + br'\\'\"' + 'it\\'s' + u\"c\"\nh = \"b\"\n";
        let spans: Vec<&str> = string_literals(source)
            .into_iter()
            .map(|(start, end)| &source[start..end])
            .collect();
        assert_eq!(spans, vec!["'it\\'s'", "u\"c\"", "\"b\""]);
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
        assert!(is_sqlty_managed("--sql\\r\\nSELECT 1"));
        assert!(is_sqlty_managed("-- sql\\nSELECT 1"));
        assert!(is_sqlty_managed("-- sql\nSELECT 1"));
        assert!(!is_sqlty_managed("-- sql query for users"));
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

    #[test]
    fn test_markers_infer() {
        for content in [
            r"--sql\nSELECT 1",
            r"--sql\r\nSELECT 1",
            r"-- sql\nSELECT 1",
            "--sql\nSELECT 1",
            "-- sql\r\nSELECT 1",
        ] {
            let inferred = infer_sql_content(content, "postgres", &None);
            assert_eq!(inferred.as_deref(), Some("tuple[int]"), "{content}");
        }
        for content in ["--sql SELECT 1", "-- sql SELECT 1", r"-- sqlx\nSELECT 1"] {
            assert_eq!(
                infer_sql_content(content, "postgres", &None),
                None,
                "{content}"
            );
        }
    }

    #[test]
    fn test_ends_marker_line() {
        for rest in ["\n", r"\n", r"\r\n", "  \n", " -- users\nSELECT 1"] {
            assert!(ends_marker_line(rest), "{rest:?}");
        }
        for rest in [
            "",
            " query notes\nSELECT 1",
            "-- users\nSELECT 1",
            " SELECT 1",
        ] {
            assert!(!ends_marker_line(rest), "{rest:?}");
        }
    }

    fn keep_lines(source: &str) -> String {
        rewrite_source_with(source, "postgres", &None, None, false, true)
    }

    const Q: &str = "q = \"--sql\\nSELECT 1::INTEGER AS x\"\n";

    #[test]
    fn test_keep_lines_script_mode_docstring() {
        let source = format!("\n\"\"\"Doc.\"\"\"  # c\n{Q}");
        assert_eq!(
            keep_lines(&source),
            "\n\"\"\"Doc.\"\"\"; from sqlty import SQL  # c\nq = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER AS x\")\n"
        );
    }

    #[test]
    fn test_keep_lines_shebang_cookie_simple_statement() {
        let source = format!("#!/usr/bin/env python\n# coding: sqlty\n{Q}");
        let result = keep_lines(&source);
        assert!(result.starts_with(
            "#!/usr/bin/env python\n# coding: sqlty\nfrom sqlty import SQL; q = SQL["
        ));
        assert!(keep_lines(&format!("\n{Q}")).starts_with("\nfrom sqlty import SQL; q = SQL["));
    }

    #[test]
    fn test_keep_lines_future() {
        let source = format!("\nfrom __future__ import (\n    annotations,\n)  # c\n{Q}");
        assert!(keep_lines(&source).starts_with(
            "\nfrom __future__ import (\n    annotations,\n); from sqlty import SQL  # c\n"
        ));
    }

    #[test]
    fn test_keep_lines_compound_statement() {
        let def = format!("\ndef f():\n    pass\n{Q}");
        assert!(keep_lines(&def).starts_with("\nfrom sqlty import SQL\ndef f():\n"));
        let blank = format!("\n\n@decorator\ndef f():\n    pass\n{Q}");
        assert!(keep_lines(&blank).starts_with("\nfrom sqlty import SQL\n@decorator\n"));
        let comment = format!("\n# c\nif x: y\n{Q}");
        assert!(keep_lines(&comment).starts_with("\nfrom sqlty import SQL  # c\nif x: y\n"));
        let doc = format!("\n\"\"\"Doc.\"\"\"\nclass A: pass\n{Q}");
        assert!(keep_lines(&doc).starts_with("\n\"\"\"Doc.\"\"\"; from sqlty import SQL\nclass A"));
    }

    #[test]
    fn test_keep_lines_docstring_semicolon_and_existing_import() {
        let doc = format!("\n\"\"\"Doc.\"\"\"; x = 1\n{Q}");
        assert!(keep_lines(&doc).starts_with("\n\"\"\"Doc.\"\"\"; from sqlty import SQL; x = 1\n"));
        let existing = format!("\nfrom sqlty import SQL\n{Q}");
        assert_eq!(
            keep_lines(&existing),
            rewrite_source(&existing, "postgres", &None, None, false)
        );
    }

    #[test]
    fn test_keep_lines_placement() {
        let imp = "from sqlty import SQL";
        for (header, expected) in [
            ("# coding: sqlty\n", format!("# coding: sqlty\n{imp}; q = ")),
            (
                "\"\"\"Multi\nline.\n\"\"\"\n",
                format!("\"\"\"Multi\nline.\n\"\"\"; {imp}\nq = "),
            ),
            (
                "\n\"\"\"Doc.\"\"\"\nfrom __future__ import annotations\n",
                format!("\n\"\"\"Doc.\"\"\"\nfrom __future__ import annotations; {imp}\nq = "),
            ),
            (
                "\n  # c\nclass A: pass\n",
                format!("\n{imp}    # c\nclass A: pass\nq = "),
            ),
            (
                "\nmatch x:\n    case _: pass\n",
                format!("\n{imp}\nmatch x:\n    case _: pass\nq = "),
            ),
            ("\nmatch = 1\n", format!("\n{imp}; match = 1\nq = ")),
        ] {
            let result = keep_lines(&format!("{header}{Q}"));
            assert!(result.starts_with(&expected), "{result:?}");
        }
        let continued = keep_lines("\nq = \\\n    \"--sql\\nSELECT 1::INTEGER AS x\"\n");
        assert!(
            continued.starts_with(&format!("\n{imp}; q = \\\n    SQL[")),
            "{continued:?}"
        );
    }

    #[test]
    fn test_keep_lines_datetime_uuid() {
        let source = "\n\"\"\"Doc.\"\"\"\r\nq = \"--sql\\nSELECT NOW()::TIMESTAMP AS t, 'a'::UUID AS u\"\r\n";
        assert!(keep_lines(source).starts_with(
            "\n\"\"\"Doc.\"\"\"; import datetime; import uuid; from sqlty import SQL\r\nq = SQL[tuple[datetime.datetime, uuid.UUID]]("
        ));
    }

    #[test]
    fn test_keep_lines_preserves_line_count() {
        for source in [
            format!("\n\"\"\"Doc.\"\"\"\n{Q}"),
            format!("\nfrom __future__ import annotations\n{Q}"),
            format!("\n\n@decorator\ndef f():\n    pass\n{Q}"),
            format!("\nimport os\n{Q}"),
            format!("\n\"\"\"Doc.\"\"\"\r\n{Q}").replace('\n', "\r\n"),
        ] {
            let result = keep_lines(&source);
            assert!(result.contains("SQL[tuple[int]]"), "{result}");
            assert_eq!(result.lines().count(), source.lines().count(), "{result}");
            assert_eq!(result.lines().next(), source.lines().next(), "{result}");
        }
    }
}
