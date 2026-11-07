//! `sqlty format`: the query strings of Python source laid out by [sqruff]'s layout rules.
//!
//! A query is formatted when it is one plain triple-quoted literal without a backslash, so
//! its text is the source text and needs no escaping. The marker line, the indentation of
//! the first line and the whitespace before the closing quotes are kept. Only whitespace
//! changes: a result that moves any other character, or a placeholder, is dropped.
//!
//! [sqruff]: https://github.com/quarylabs/sqruff
//!
//! ```
//! use sqlty_format::format_source;
//! use sqlty_python::rewriter::ImportStyle;
//!
//! let style = ImportStyle::parse(None).unwrap();
//! let source = "q = \"\"\"--sql\nselect id,name from users where id=%s\"\"\"\n";
//! assert_eq!(
//!     format_source(source, "postgres", &style),
//!     "q = \"\"\"--sql\nselect\n    id,\n    name\nfrom users\nwhere id = %s\"\"\"\n",
//! );
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;
use sqlty_python::edit::{apply_edits, Edit};
use sqlty_python::locate::Queries;
use sqlty_python::rewriter::ImportStyle;
use sqruff_lib::core::config::FluffConfig;
use sqruff_lib::core::linter::core::Linter;

/// psycopg's `%s`, `%(name)s` and `%%`, the `?` of SQLite and MySQL drivers, and the `{name}`
/// fields of `str.format` and `sql.SQL` templates, which sqruff does not read as values.
static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"%(?:\([^)]*\))?[sbt]|%%|\?|\{\w*\}").unwrap());

/// `source` with every query string sqruff can lay out formatted in `dialect`.
pub fn format_source(source: &str, dialect: &str, style: &ImportStyle) -> String {
    with_linter(dialect, "rules = layout", |linter| {
        format_with(linter, source, style)
    })
}

fn format_with(linter: &Linter, source: &str, style: &ImportStyle) -> String {
    let queries = Queries::new(source, style, &[]);
    let edits = queries
        .literals()
        .filter_map(|literals| {
            let [literal] = literals else {
                return None;
            };
            let text = &source[literal.clone()];
            let body = ["\"\"\"", "'''"]
                .iter()
                .find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
                .filter(|body| !body.contains('\\'))?;
            let start = literal.start + 3;
            let marker = if body.trim_start().starts_with("--") {
                body.find('\n').map_or(body.len(), |n| n + 1)
            } else {
                0
            };
            let query = &body[marker..];
            let formatted = format_query(linter, query)?;
            (formatted != query).then(|| Edit::new(start + marker..start + body.len(), formatted))
        })
        .collect();
    apply_edits(source, edits).0
}

/// A finding of sqruff's lint rules in a query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The sqruff rule, such as `AM01`.
    pub code: &'static str,
    /// What sqruff says is wrong.
    pub message: String,
    /// The source bytes it is about.
    pub span: Range<usize>,
    /// Where the query's string starts, the line a `# sqlty: ignore` comment is on.
    pub query_start: usize,
}

/// What sqruff's core rules, less its layout rules (`sqlty format` owns layout), find in
/// the query strings of `source`, in `dialect`. A finding on a placeholder is dropped.
///
/// ```
/// use sqlty_format::lint_source;
/// use sqlty_python::rewriter::ImportStyle;
///
/// let style = ImportStyle::parse(None).unwrap();
/// let source = "q = \"--sql\\nSELECT a FROM t GROUP BY 1 ORDER BY a\"\n";
/// let [found] = &lint_source(source, "postgres", &style)[..] else { panic!() };
/// assert_eq!(found.code, "AM06");
/// assert_eq!(&source[found.span.clone()], "ORDER BY a");
/// ```
pub fn lint_source(source: &str, dialect: &str, style: &ImportStyle) -> Vec<Finding> {
    with_linter(dialect, "rules = core\nexclude_rules = layout", |linter| {
        lint_with(linter, source, style)
    })
}

fn lint_with(linter: &Linter, source: &str, style: &ImportStyle) -> Vec<Finding> {
    let queries = Queries::new(source, style, &[]);
    let mut found = Vec::new();
    for literals in queries.literals() {
        // Inside the closing quote, one or three of them.
        let Some(query) = literals.last().and_then(|last| {
            queries
                .at(last.end - 1)
                .or_else(|| queries.at(last.end - 3))
        }) else {
            continue;
        };
        // Same length, so offsets in the query hold.
        let protected = PLACEHOLDER.replace_all(&query.text, |found: &regex::Captures| {
            "p".repeat(found[0].len())
        });
        let placeholders: Vec<_> = PLACEHOLDER
            .find_iter(&query.text)
            .map(|m| m.range())
            .collect();
        let Ok(linted) = linter.lint_string(&protected, None, false) else {
            continue;
        };
        for violation in linted.violations() {
            let slice = &violation.source_slice;
            let on_placeholder = placeholders.iter().any(|placeholder| {
                placeholder.start < slice.end.max(slice.start + 1) && slice.start < placeholder.end
            });
            let span = query
                .source_offset(slice.start)
                .zip(query.source_offset(slice.end));
            if let (false, Some((start, end))) = (on_placeholder, span) {
                found.push(Finding {
                    code: violation.rule_code(),
                    message: violation.desc().to_string(),
                    span: start..end,
                    query_start: literals[0].start,
                });
            }
        }
    }
    found
}

/// `query` laid out, at the indentation of its first line, with its surrounding whitespace
/// kept; `None` when sqruff changes more than whitespace.
fn format_query(linter: &Linter, query: &str) -> Option<String> {
    let trimmed = query.trim_end();
    let body = trimmed.trim_start_matches(['\r', '\n']);
    let lead = &query[..trimmed.len() - body.len()];
    let trail = &query[trimmed.len()..];
    let indent = &body[..body.len() - body.trim_start().len()];
    if body.trim().is_empty() || indent.contains('\n') {
        return None;
    }
    let dedented = body
        .lines()
        .map(|line| line.strip_prefix(indent).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let mut values = Vec::new();
    let protected = PLACEHOLDER.replace_all(&dedented, |found: &regex::Captures| {
        values.push(found[0].to_string());
        format!("__sqlty_{}__", values.len() - 1)
    });
    let fixed = linter
        .lint_string(&protected, None, true)
        .ok()?
        .fix_string();
    let mut restored = fixed.trim_end().to_string();
    for (i, value) in values.iter().enumerate() {
        let name = format!("__sqlty_{i}__");
        if restored.matches(&name).count() != 1 {
            return None;
        }
        restored = restored.replace(&name, value);
    }
    let words = |text: &str| -> String { text.split_whitespace().collect() };
    if words(&restored) != words(body) {
        return None;
    }
    let indented: Vec<String> = restored
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect();
    Some(format!("{lead}{}{trail}", indented.join("\n")))
}

/// Call `f` with a linter of the sqruff `rules` setting in `dialect`, built once per thread:
/// its rules take a second to build.
fn with_linter<T>(dialect: &str, rules: &'static str, f: impl FnOnce(&Linter) -> T) -> T {
    thread_local! {
        static LINTERS: RefCell<HashMap<(String, &'static str), Rc<Linter>>> = RefCell::default();
    }
    let linter = LINTERS.with_borrow_mut(|linters| {
        Rc::clone(
            linters
                .entry((dialect.to_string(), rules))
                .or_insert_with(|| Rc::new(linter(dialect, rules))),
        )
    });
    f(&linter)
}

/// A linter with the sqruff `rules` setting, in `dialect` or ANSI SQL when sqruff has no
/// such dialect. It reads no configuration file.
fn linter(dialect: &str, rules: &str) -> Linter {
    let config = |dialect: &str| {
        FluffConfig::try_from_source(
            &format!("[sqruff]\ndialect = {dialect}\ntemplater = raw\n{rules}\n"),
            None,
        )
        .ok()
    };
    let config = config(dialect)
        .or_else(|| config("ansi"))
        .expect("sqruff knows ANSI SQL");
    Linter::new(config, None, None, false).expect("the raw templater exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint(source: &str) -> Vec<(&'static str, &str)> {
        let style = ImportStyle::parse(None).unwrap();
        lint_source(source, "postgres", &style)
            .into_iter()
            .map(|found| (found.code, &source[found.span]))
            .collect()
    }

    #[test]
    fn a_finding_is_mapped_through_escapes() {
        let source = "q = \"--sql\\n\\tselect 'x\\\\y', distinct(a) from t t1\"\n";
        assert_eq!(
            lint(source),
            [
                ("AL03", "'x\\\\y'"),
                ("AL03", "distinct(a)"),
                ("AL05", "t1")
            ]
        );
    }

    #[test]
    fn a_triple_quoted_query_is_linted() {
        let source = "q = \"\"\"--sql\nselect a from t\ngroup by 1 order by a\"\"\"\n";
        assert_eq!(lint(source), [("AM06", "order by a")]);
    }

    #[test]
    fn placeholders_and_layout_are_not_reported() {
        let source =
            "q = \"--sql\\nSELECT a FROM t JOIN u ON t.id = u.id WHERE x = %(x)s AND y=?\"\n";
        assert_eq!(lint(source), []);
    }
}
