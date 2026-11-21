//! The mdtest assertions that need sqruff: `# formatted:` and the diagnostics of sections that
//! turn the `sqruff` rule on, see `rust/mdtest/python.rs`.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test reads the suite's files and fails on a malformed one"
)]
#![allow(dead_code, reason = "the shared runner serves every crate's kinds")]

#[path = "../../../mdtest/harness.rs"]
mod harness;
#[path = "../../../mdtest/parser.rs"]
mod parser;
#[path = "../../../mdtest/python.rs"]
mod python;
#[path = "../../../mdtest/sql.rs"]
mod sql;

use sqlty_python::check;
use sqlty_python::rewriter::ImportStyle;

fn lint(text: &str, dialect: &str, style: &ImportStyle) -> Vec<check::Lint> {
    sqlty_format::lint_source(text, dialect, style)
        .into_iter()
        .map(|found| check::Lint {
            message: format!("{}: {}", found.code, found.message),
            span: found.span,
            query_start: found.query_start,
        })
        .collect()
}

#[test]
fn mdtest() {
    harness::run(&python::Python {
        sqruff: Some((lint, sqlty_format::format_source)),
    });
}
