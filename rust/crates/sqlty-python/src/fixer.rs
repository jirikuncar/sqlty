//! `sqlty check --fix`: apply the fixable rules to Python source.
//!
//! User-defined row types (`SQL[User]`) are never changed.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use regex::Regex;
use std::sync::LazyLock;

use crate::checker::check_source;
use crate::edit::apply_edits;
use crate::options::Options;
use crate::rewriter::{
    has_import, insert_imports, suppresses, type_modules, with_wrapper_imports, ImportStyle,
    Spelling, Suppressed,
};
use crate::rules::{Rule, RuleConfig};
use crate::scan::Scan;

/// The rules whose diagnostics carry a fix.
const FIXABLE: [Rule; 6] = [
    Rule::InlineSqlComment,
    Rule::MysqlSqlMarker,
    Rule::StaleRewrite,
    Rule::SelectStar,
    Rule::StaleType,
    Rule::UntypedQuery,
];

/// Fixing one problem can reveal another (a `"--sql SELECT"` fixed into a query is then
/// wrapped); a pass applies every fix that does not overlap another, up to this many.
const MAX_PASSES: usize = 8;

/// `\bname\b`, compiled once per name: the few import styles in use repeat call after call.
fn word_regex(name: &str) -> Regex {
    static CACHE: LazyLock<Mutex<HashMap<String, Regex>>> = LazyLock::new(Mutex::default);
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(name.to_string())
        .or_insert_with(|| Regex::new(&format!(r"\b{}\b", regex::escape(name))).unwrap())
        .clone()
}

/// `check_source` plus the edits of its diagnostics: fix `inline-sql-comment`,
/// `mysql-sql-marker`, `stale-rewrite`, `select-star`, `stale-type` and `untyped-query`,
/// except where a `# sqlty: ignore` comment on the line the rule reports (`suppressed`, by
/// line of `source`) suppresses that rule. The imports the fixes need are added last, so
/// the lines of `suppressed` hold for every pass.
///
/// User-defined row types (`SQL[User]`) are never changed.
pub fn fix_source(
    source: &str,
    options: &Options,
    rules: &RuleConfig,
    suppressed: &Suppressed,
) -> String {
    let rules = rules.only(&FIXABLE);
    let mut fixed = source.to_string();
    let mut wrapped = Vec::new();
    let mut introduced = BTreeSet::new();
    for _ in 0..MAX_PASSES {
        let edits: Vec<_> = check_source(&fixed, options, &rules, &[])
            .into_iter()
            .filter(|diag| !suppresses(suppressed, diag.line, diag.rule))
            .flat_map(|diag| diag.fix.into_iter().map(move |edit| (diag.rule, edit)))
            .collect();
        if edits.is_empty() {
            break;
        }
        let rule_at: HashMap<_, _> = edits
            .iter()
            .map(|(rule, edit)| ((edit.range.start, edit.range.end), *rule))
            .collect();
        let (next, applied) = apply_edits(&fixed, edits.into_iter().map(|(_, e)| e).collect());
        for edit in applied {
            match rule_at[&(edit.range.start, edit.range.end)] {
                Rule::StaleRewrite => wrapped.push(edit.replacement),
                Rule::StaleType | Rule::UntypedQuery => {
                    introduced.insert(edit.replacement);
                }
                _ => {}
            }
        }
        if next == fixed {
            break;
        }
        fixed = next;
    }
    if !wrapped.is_empty() {
        let scan = Scan::new(&fixed, &options.style.wrapper_name);
        let wrapped: Vec<&str> = wrapped.iter().map(String::as_str).collect();
        fixed = with_wrapper_imports(
            fixed.clone(),
            &scan,
            &wrapped,
            &options.style,
            &type_modules(options),
            false,
        );
    }
    if fixed == source {
        return fixed;
    }
    fix_imports(fixed, &options.style, &type_modules(options), &introduced)
}

/// Add the `modules` imports new row types need; drop an `UNTYPED` import no longer used.
fn fix_imports(
    mut source: String,
    style: &ImportStyle,
    modules: &[String],
    introduced: &BTreeSet<String>,
) -> String {
    let mut missing: Vec<String> = modules
        .iter()
        .filter(|module| {
            introduced
                .iter()
                .any(|ty| ty.contains(&format!("{module}.")))
        })
        .map(|module| format!("import {module}"))
        .filter(|line| !has_import(&source, line))
        .collect();
    let wrapped: Vec<String> = introduced.iter().map(|ty| format!("[{ty}](")).collect();
    let wrapped: Vec<&str> = wrapped.iter().map(String::as_str).collect();
    missing.extend(
        Spelling::of(&Scan::new(source.as_str(), &style.wrapper_name)).imports_for(&wrapped),
    );
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
        source = insert_imports(&source, &missing);
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(source: &str) -> String {
        let options = Options::new("postgres", None, None, false).unwrap();
        fix_source(source, &options, &RuleConfig::default(), &Suppressed::new())
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
            fix(r#"a = "--sql SELECT 1::INTEGER"
b = f"--sql SELECT 1""#),
            r#"from sqlty import SQL
a = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
b = f"--sql SELECT 1""#
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
        let options = Options::new("mysql", None, None, false).unwrap();
        let rules = RuleConfig::from_overrides(Some(
            [
                ("stale-rewrite".to_string(), "ignore".to_string()),
                ("inline-sql-comment".to_string(), "ignore".to_string()),
            ]
            .into(),
        ))
        .unwrap();
        assert_eq!(
            fix_source(source, &options, &rules, &Suppressed::new()),
            fixed
        );
    }
}
