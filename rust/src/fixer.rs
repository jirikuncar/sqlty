//! `sqlty check --fix`: apply the fixable rules to Python source.
//!
//! User-defined row types (`SQL[User]`) are never changed.

use std::collections::{BTreeSet, HashMap};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::analyzer::{expand_star, Schema};
use crate::checker::{refines_tuple, RuleConfig, INLINE_SQL_COMMENT_CONTENT};
use crate::rewriter::{
    ends_marker_line, extract_string_content, has_import, header_end, infer_sql_content,
    is_sqlty_managed, line_ending, rewrite_source, splice, string_literals, ImportStyle,
};
use crate::tokens::{tokenize, wrapper_calls};

const QUOTES: [&str; 4] = ["\"\"\"", "'''", "\"", "'"];

/// A `--sql` marker line at the start of string content.
static SQL_PREFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)^(\s*(?:--sql|-- sql)(?:\\r\\n|\\n|\r?\n))(.*)").unwrap());

/// Fix `inline-sql-comment`, `mysql-sql-marker`, `stale-rewrite`, `select-star`,
/// `stale-type` and `untyped-query`.
pub fn fix_source(
    source: &str,
    dialect: &str,
    schema: &Option<Schema>,
    import_style: Option<&str>,
    untyped: bool,
    rules: Option<HashMap<String, String>>,
) -> String {
    let rules = RuleConfig::new(rules);
    let enabled = |rule: &str| rules.is_enabled(rule);
    let style = ImportStyle::or_default(import_style);
    let mut source = source.to_string();
    if enabled("inline-sql-comment") {
        source = fix_inline_sql_comments(&source);
    }
    if dialect.eq_ignore_ascii_case("mysql") && enabled("mysql-sql-marker") {
        source = fix_mysql_markers(&source);
    }
    if enabled("stale-rewrite") {
        source = rewrite_source(&source, dialect, schema, import_style, untyped);
    }

    let untyped_name = style.untyped_name();
    let mut introduced = BTreeSet::new();
    let mut edits = Vec::new();
    for call in wrapper_calls(&source, &tokenize(&source), &style.wrapper_name) {
        let Some(mut content) = call.content(&source) else {
            continue;
        };
        if !is_sqlty_managed(&content) {
            continue;
        }
        let mut literal_edit = None;
        if let ([span], Some(schema)) = (
            call.strings.as_slice(),
            schema.as_ref().filter(|_| enabled("select-star")),
        ) {
            let literal = &source[span.clone()];
            let quoted = literal.trim_start_matches(['u', 'U']);
            let prefix = &literal[..literal.len() - quoted.len()];
            let quote = QUOTES
                .into_iter()
                .find(|q| quoted.starts_with(q))
                .unwrap_or("\"");
            let expanded = expand_star_in(&content, quote, dialect, schema);
            if expanded != content {
                literal_edit = Some((
                    span.start,
                    span.end,
                    format!("{prefix}{quote}{expanded}{quote}"),
                ));
                content = expanded;
            }
        }
        let annotated = &source[call.annotation.clone()];
        let refresh = (annotated.starts_with("tuple[") && enabled("stale-type"))
            || (annotated == untyped_name && enabled("untyped-query"));
        if let Some(row_type) = refresh
            .then(|| infer_sql_content(&content, dialect, schema))
            .flatten()
            .filter(|inferred| inferred != annotated && !refines_tuple(annotated, inferred))
        {
            edits.push((call.annotation.start, call.annotation.end, row_type.clone()));
            introduced.insert(row_type);
        }
        edits.extend(literal_edit);
    }
    let fixed = splice(&source, &edits);
    if fixed == source {
        return source;
    }
    fix_imports(fixed, &style, &introduced)
}

/// `"--sql SELECT ..."` to `"--sql\nSELECT ..."` in plain string literals that
/// `inline-sql-comment` reports, so prose such as `"--sql flag enables it"` is kept.
/// This and `select-star` are the only fixes that change string content.
fn fix_inline_sql_comments(source: &str) -> String {
    let edits: Vec<(usize, usize, String)> = string_literals(source)
        .into_iter()
        .filter(|&(start, end)| {
            INLINE_SQL_COMMENT_CONTENT.is_match(extract_string_content(&source[start..end]))
        })
        .map(|(start, end)| {
            let literal = &source[start..end];
            let quoted = literal.trim_start_matches(['u', 'U']);
            let quote = QUOTES
                .into_iter()
                .find(|q| quoted.starts_with(q))
                .unwrap_or("\"");
            let at = end - quoted.len() + quote.len() + "--sql".len();
            (at, at + 1, "\\n".to_string())
        })
        .collect();
    splice(source, &edits)
}

/// `"--sql\n..."` to `"-- sql\n..."`, the marker MySQL reads as a comment.
fn fix_mysql_markers(source: &str) -> String {
    let edits: Vec<(usize, usize, String)> = string_literals(source)
        .into_iter()
        .filter_map(|(start, end)| {
            let literal = &source[start..end];
            let content = extract_string_content(literal);
            let marked = content.trim_start();
            marked
                .strip_prefix("--sql")
                .is_some_and(ends_marker_line)
                .then(|| {
                    let at = start + literal.find(marked).unwrap_or_default();
                    (at, at + "--sql".len(), "-- sql".to_string())
                })
        })
        .collect();
    splice(source, &edits)
}

/// Replace `*` in the query part of a string literal, when that is safe to write back.
fn expand_star_in(content: &str, quote: &str, dialect: &str, schema: &Schema) -> String {
    let Some(caps) = SQL_PREFIX.captures(content) else {
        return content.to_string();
    };
    let (prefix, query) = (&caps[1], &caps[2]);
    let quote_char = &quote[..1];
    if !query.contains('*') || query.contains('\\') || query.contains(quote_char) {
        return content.to_string();
    }
    match expand_star(query, dialect, &Some(schema.clone())) {
        Some(expanded)
            if !expanded.contains('\\')
                && !expanded.contains(quote_char)
                && !(quote.len() == 1 && expanded.contains('\n')) =>
        {
            format!("{prefix}{expanded}")
        }
        _ => content.to_string(),
    }
}

/// Add `datetime`/`uuid` imports new row types need; drop an `UNTYPED` import no longer used.
fn fix_imports(mut source: String, style: &ImportStyle, introduced: &BTreeSet<String>) -> String {
    let missing: Vec<String> = ["datetime", "uuid"]
        .into_iter()
        .filter(|module| {
            introduced
                .iter()
                .any(|ty| ty.contains(&format!("{module}.")))
        })
        .map(|module| format!("import {module}"))
        .filter(|line| !has_import(&source, line))
        .collect();
    if let Some(untyped) = style
        .untyped_import()
        .filter(|line| has_import(&source, line))
    {
        let rest: String = source
            .split_inclusive('\n')
            .filter(|line| line.trim_end() != untyped)
            .collect();
        let used = Regex::new(&format!(r"\b{}\b", regex::escape(&style.untyped_name())))
            .unwrap()
            .is_match(&rest);
        if !used {
            source = rest;
        }
    }
    if !missing.is_empty() {
        let at = header_end(&source);
        let eol = line_ending(&source, at);
        source = format!(
            "{}{}{eol}{}",
            &source[..at],
            missing.join(eol),
            &source[at..]
        );
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(source: &str) -> String {
        fix_source(source, "postgres", &None, None, false, None)
    }

    #[test]
    fn test_stale_type_updated() {
        assert_eq!(
            fix("from sqlty import SQL\nq = SQL[tuple[str]](\"--sql\\nSELECT 1::INTEGER\")"),
            "from sqlty import SQL\nq = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER\")"
        );
    }

    #[test]
    fn test_wrapper_argument_spans_to_closing_paren() {
        assert_eq!(
            fix("q = SQL[tuple[str]](\"--sql\\nSELECT 1::INTEGER \"\n  \"AS n\")"),
            "q = SQL[tuple[int]](\"--sql\\nSELECT 1::INTEGER \"\n  \"AS n\")"
        );
        assert_eq!(
            fix("q = SQL[tuple[str]](\"\"\"--sql\nSELECT 1::INTEGER AS n\"\"\"  # c\n)"),
            "q = SQL[tuple[int]](\"\"\"--sql\nSELECT 1::INTEGER AS n\"\"\"  # c\n)"
        );
    }

    #[test]
    fn test_inline_comment_only_in_plain_strings() {
        assert_eq!(
            fix_inline_sql_comments(r#"a = "--sql SELECT 1" b = f"--sql SELECT 1""#),
            r#"a = "--sql\nSELECT 1" b = f"--sql SELECT 1""#
        );
    }

    #[test]
    fn test_user_types_untouched() {
        let source = "from sqlty import SQL\nq = SQL[User](\"--sql\\nSELECT 1::INTEGER\")";
        assert_eq!(fix(source), source);
    }

    #[test]
    fn test_datetime_import_added() {
        assert_eq!(
            fix("from sqlty import SQL\nq = SQL[tuple[str]](\"--sql\\nSELECT now()\")"),
            "import datetime\nfrom sqlty import SQL\nq = SQL[tuple[datetime.datetime]](\"--sql\\nSELECT now()\")"
        );
    }

    #[test]
    fn test_datetime_import_uses_crlf() {
        assert_eq!(
            fix("from sqlty import SQL\r\nq = SQL[tuple[str]](\"--sql\\nSELECT now()\")\r\n"),
            "import datetime\r\nfrom sqlty import SQL\r\nq = SQL[tuple[datetime.datetime]](\"--sql\\nSELECT now()\")\r\n"
        );
    }

    #[test]
    fn test_mysql_markers_fixed_in_plain_literals() {
        let source = r#"a = "--sql\nSELECT 1"
b = """  --sql
SELECT 1"""
c = f"--sql\nSELECT 1"
d = "--sql SELECT 1""#;
        let fixed = r#"a = "-- sql\nSELECT 1"
b = """  -- sql
SELECT 1"""
c = f"--sql\nSELECT 1"
d = "--sql SELECT 1""#;
        assert_eq!(fix_mysql_markers(source), fixed);
    }
}
