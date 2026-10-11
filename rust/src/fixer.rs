//! `sqlty check --fix`: apply the fixable rules to Python source.
//!
//! User-defined row types (`SQL[User]`) are never changed.

use std::collections::{BTreeSet, HashMap};

use once_cell::sync::Lazy;
use regex::{Captures, Regex};

use crate::analyzer::{expand_star, Schema};
use crate::checker::RuleConfig;
use crate::rewriter::{
    extract_string_content, has_import, header_end, infer_sql_content, is_sqlty_managed,
    rewrite_source, ImportStyle,
};

/// `"--sql SELECT ..."` opening a plain string literal; raw, f- and t-strings are left alone.
static INLINE_SQL_COMMENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?P<quote>[uU]?(?:"""|'''|"|'))--sql (?P<next>\S)"#).unwrap());

/// A `--sql` marker line at the start of string content.
static SQL_PREFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)^(\s*--sql(?:\\n|\n))(.*)").unwrap());

/// Fix `inline-sql-comment`, `stale-rewrite`, `select-star`, `stale-type` and `untyped-query`.
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
    if enabled("stale-rewrite") {
        source = rewrite_source(&source, dialect, schema, import_style, untyped);
    }

    let untyped_name = style.untyped_name();
    let mut introduced = BTreeSet::new();
    let fixed = wrapper_pattern(&style)
        .replace_all(&source, |caps: &Captures| {
            let (annotated, literal) = (&caps["type"], &caps["inner"]);
            let quote = ["\"\"\"", "'''", "\"", "'"]
                .into_iter()
                .find(|q| literal.starts_with(q))
                .unwrap_or("\"");
            let mut content = extract_string_content(literal).to_string();
            if !is_sqlty_managed(&content) {
                return caps[0].to_string();
            }
            if let Some(schema) = schema.as_ref().filter(|_| enabled("select-star")) {
                content = expand_star_in(&content, quote, dialect, schema);
            }
            let refresh = (annotated.starts_with("tuple[") && enabled("stale-type"))
                || (annotated == untyped_name && enabled("untyped-query"));
            let row_type = refresh
                .then(|| infer_sql_content(&content, dialect, schema))
                .flatten()
                .unwrap_or_else(|| annotated.to_string());
            if row_type != annotated {
                introduced.insert(row_type.clone());
            }
            format!(
                "{}[{row_type}]({}{quote}{content}{quote}{}",
                style.wrapper_name, &caps["space"], &caps["end"]
            )
        })
        .into_owned();
    if fixed == source {
        return source;
    }
    fix_imports(fixed, &style, &introduced)
}

fn fix_inline_sql_comments(source: &str) -> String {
    INLINE_SQL_COMMENT
        .replace_all(source, |caps: &Captures| {
            let start = caps.get(0).map_or(0, |m| m.start());
            let after_word = source[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if after_word {
                caps[0].to_string()
            } else {
                format!("{}--sql\\n{}", &caps["quote"], &caps["next"])
            }
        })
        .into_owned()
}

fn wrapper_pattern(style: &ImportStyle) -> Regex {
    Regex::new(&format!(
        r#"(?s){}\[(?P<type>[^\n]*?)\]\((?P<space>\s*)(?P<inner>""".*?"""|'''.*?'''|"[^"]*"|'[^']*')(?P<end>\s*,?\s*\))"#,
        regex::escape(&style.wrapper_name)
    ))
    .unwrap()
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
        source = format!("{}{}\n{}", &source[..at], missing.join("\n"), &source[at..]);
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
}
