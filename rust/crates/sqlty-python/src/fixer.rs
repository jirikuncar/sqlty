//! `sqlty check --fix`: apply the fixable rules to Python source.
//!
//! User-defined row types (`SQL[User]`) are never changed.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use regex::Regex;
use std::sync::LazyLock;

use crate::checker::{refines_tuple, INLINE_SQL_COMMENT_CONTENT};
use crate::options::Options;
use crate::rewriter::{
    ends_marker_line, extract_string_content, has_import, header_end, infer_sql_content,
    is_sqlty_managed, line_ending, rewrite_scan, splice, suppresses, ImportStyle, LineIndex,
    Suppressed, TYPE_MODULES,
};
use crate::rules::{Rule, RuleConfig};
use crate::scan::Scan;
use sqlty_sql::analyzer::{expand_star, SchemaData};

const QUOTES: [&str; 4] = ["\"\"\"", "'''", "\"", "'"];

/// `\bname\b`, compiled once per name: the few import styles in use repeat call after call.
fn word_regex(name: &str) -> Regex {
    static CACHE: LazyLock<Mutex<HashMap<String, Regex>>> = LazyLock::new(Mutex::default);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(name.to_string())
        .or_insert_with(|| Regex::new(&format!(r"\b{}\b", regex::escape(name))).unwrap())
        .clone()
}

/// A `--sql` marker line at the start of string content.
static SQL_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^(\s*(?:--sql|-- sql)(?:\\r\\n|\\n|\r?\n))(.*)").unwrap());

/// Fix `inline-sql-comment`, `mysql-sql-marker`, `stale-rewrite`, `select-star`,
/// `stale-type` and `untyped-query`, except where a `# sqlty: ignore` comment on the line
/// the rule reports (`suppressed`, by line of `source`) suppresses that rule.
pub fn fix_source(
    source: &str,
    options: &Options,
    rules: &RuleConfig,
    mut suppressed: Suppressed,
) -> String {
    let enabled = |rule: Rule| rules.is_enabled(rule);
    let (dialect, schema) = (options.dialect.name(), options.schema);
    let style = &options.style;
    let wrapper = &style.wrapper_name;
    // Each phase reads one scan; the source is scanned again only after a phase edits it.
    let mut scan = Scan::new(source, wrapper);
    if enabled(Rule::InlineSqlComment) {
        if let Some(fixed) = fix_inline_sql_comments(&scan, &suppressed) {
            scan = Scan::new(fixed, wrapper);
        }
    }
    if options.dialect.is_mysql() && enabled(Rule::MysqlSqlMarker) {
        if let Some(fixed) = fix_mysql_markers(&scan, &suppressed) {
            scan = Scan::new(fixed, wrapper);
        }
    }
    if enabled(Rule::StaleRewrite) {
        let skip = |line| suppresses(&suppressed, line, Rule::StaleRewrite);
        if let Some(rewritten) = rewrite_scan(&scan, options, false, &skip) {
            suppressed = shift_lines(suppressed, scan.source(), &rewritten);
            scan = Scan::new(rewritten, wrapper);
        }
    }
    let source = scan.source();
    let allowed = |rule: Rule, call_start: usize| {
        enabled(rule) && !suppresses(&suppressed, scan.lines.line_at(call_start), rule)
    };

    let untyped_name = style.untyped_name();
    let mut introduced = BTreeSet::new();
    let mut edits = Vec::new();
    for call in &scan.calls {
        let Some(mut content) = call.content(source) else {
            continue;
        };
        if !is_sqlty_managed(&content) {
            continue;
        }
        let mut literal_edit = None;
        if let ([span], Some(schema)) = (
            call.strings.as_slice(),
            schema
                .as_ref()
                .filter(|_| allowed(Rule::SelectStar, call.span.start)),
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
        let refresh = (annotated.starts_with("tuple[")
            && allowed(Rule::StaleType, call.span.start))
            || (annotated == untyped_name && allowed(Rule::UntypedQuery, call.span.start));
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
    let fixed = splice(source, &edits);
    if fixed == source {
        return scan.into_source().into_owned();
    }
    fix_imports(fixed, style, &introduced)
}

/// `"--sql SELECT ..."` to `"--sql\nSELECT ..."` in plain string literals that
/// `inline-sql-comment` reports, so prose such as `"--sql flag enables it"` is kept.
/// This and `select-star` are the only fixes that change string content.
/// `None` when there is nothing to fix.
fn fix_inline_sql_comments(scan: &Scan, suppressed: &Suppressed) -> Option<String> {
    let source = scan.source();
    let edits: Vec<(usize, usize, String)> = scan
        .plain_literals()
        .filter(|&(start, end)| {
            INLINE_SQL_COMMENT_CONTENT.is_match(extract_string_content(&source[start..end]))
        })
        .filter(|&(start, _)| {
            let reported = if suppressed.is_empty() {
                start
            } else {
                scan.calls
                    .enclosing(start, start + 1)
                    .map_or(start, |call| call.span.start)
            };
            !suppresses(
                suppressed,
                scan.lines.line_at(reported),
                Rule::InlineSqlComment,
            )
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
    (!edits.is_empty()).then(|| splice(source, &edits))
}

/// `"--sql\n..."` to `"-- sql\n..."`, the marker MySQL reads as a comment.
/// `None` when there is nothing to fix.
fn fix_mysql_markers(scan: &Scan, suppressed: &Suppressed) -> Option<String> {
    let source = scan.source();
    let edits: Vec<(usize, usize, String)> = scan
        .plain_literals()
        .filter_map(|(start, end)| {
            let literal = &source[start..end];
            let content = extract_string_content(literal);
            let marked = content.trim_start();
            if suppresses(suppressed, scan.lines.line_at(start), Rule::MysqlSqlMarker) {
                return None;
            }
            marked
                .strip_prefix("--sql")
                .is_some_and(ends_marker_line)
                .then(|| {
                    let at = start + literal.find(marked).unwrap_or_default();
                    (at, at + "--sql".len(), "-- sql".to_string())
                })
        })
        .collect();
    (!edits.is_empty()).then(|| splice(source, &edits))
}

/// `suppressed` moved to the line numbers of `after`, the same source with `import` lines
/// added at its header.
fn shift_lines(suppressed: Suppressed, before: &str, after: &str) -> Suppressed {
    let added = after
        .matches('\n')
        .count()
        .saturating_sub(before.matches('\n').count());
    if added == 0 || suppressed.is_empty() {
        return suppressed;
    }
    let first = LineIndex::new(before).line_at(header_end(before));
    suppressed
        .into_iter()
        .map(|(line, rules)| (if line >= first { line + added } else { line }, rules))
        .collect()
}

/// Replace `*` in the query part of a string literal, when that is safe to write back.
fn expand_star_in(content: &str, quote: &str, dialect: &str, schema: &SchemaData) -> String {
    let Some(caps) = SQL_PREFIX.captures(content) else {
        return content.to_string();
    };
    let (prefix, query) = (&caps[1], &caps[2]);
    let quote_char = &quote[..1];
    if !query.contains('*') || query.contains('\\') || query.contains(quote_char) {
        return content.to_string();
    }
    match expand_star(query, dialect, Some(schema)) {
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

/// Add the [`TYPE_MODULES`] imports new row types need; drop an `UNTYPED` import no longer used.
fn fix_imports(mut source: String, style: &ImportStyle, introduced: &BTreeSet<String>) -> String {
    let missing: Vec<String> = TYPE_MODULES
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
        let used = word_regex(&style.untyped_name()).is_match(&rest);
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
        let options = Options::new("postgres", None, None, false).unwrap();
        fix_source(source, &options, &RuleConfig::default(), Suppressed::new())
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
            fix_inline_sql_comments(
                &Scan::new(r#"a = "--sql SELECT 1" b = f"--sql SELECT 1""#, "SQL"),
                &Suppressed::new()
            )
            .as_deref(),
            Some(r#"a = "--sql\nSELECT 1" b = f"--sql SELECT 1""#)
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
        assert_eq!(
            fix_mysql_markers(&Scan::new(source, "SQL"), &Suppressed::new()).as_deref(),
            Some(fixed)
        );
    }
}
