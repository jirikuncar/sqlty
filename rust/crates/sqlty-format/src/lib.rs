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

use std::sync::LazyLock;

use regex::Regex;
use sqlty_python::edit::{apply_edits, Edit};
use sqlty_python::locate::Queries;
use sqlty_python::rewriter::ImportStyle;
use sqruff_lib::core::config::FluffConfig;
use sqruff_lib::core::linter::core::Linter;

/// psycopg's `%s`, `%(name)s` and `%%`, and the `?` of SQLite and MySQL drivers, which
/// sqruff does not read as values.
static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"%(?:\([^)]*\))?[sbt]|%%|\?").unwrap());

/// `source` with every query string sqruff can lay out formatted in `dialect`.
pub fn format_source(source: &str, dialect: &str, style: &ImportStyle) -> String {
    let linter = linter(dialect);
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
            let formatted = format_query(&linter, query)?;
            (formatted != query).then(|| Edit::new(start + marker..start + body.len(), formatted))
        })
        .collect();
    apply_edits(source, edits).0
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

/// A linter with sqruff's layout rules, in `dialect` or ANSI SQL when sqruff has no such
/// dialect. It reads no configuration file.
fn linter(dialect: &str) -> Linter {
    let config = |dialect: &str| {
        FluffConfig::try_from_source(
            &format!("[sqruff]\ndialect = {dialect}\ntemplater = raw\nrules = layout\n"),
            None,
        )
        .ok()
    };
    let config = config(dialect)
        .or_else(|| config("ansi"))
        .expect("sqruff knows ANSI SQL");
    Linter::new(config, None, None, false).expect("the raw templater exists")
}
