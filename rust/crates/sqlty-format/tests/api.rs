//! `format_source` and `lint_source`: what the layout and the lint report on query strings.

#![expect(
    clippy::expect_used,
    reason = "a test fails on the first unexpected error"
)]

use sqlty_format::{format_source, lint_source};
use sqlty_python::rewriter::ImportStyle;

fn style() -> ImportStyle {
    ImportStyle::parse(None).expect("the default style parses")
}

fn lint(source: &str) -> Vec<(&'static str, &str)> {
    lint_source(source, "postgres", &style())
        .into_iter()
        .map(|found| (found.code, &source[found.span]))
        .collect()
}

fn format(source: &str) -> String {
    format_source(source, "postgres", &style())
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
    let source = "q = \"--sql\\nSELECT a FROM t JOIN u ON t.id = u.id WHERE x = %(x)s AND y=?\"\n";
    assert_eq!(lint(source), []);
}

#[test]
fn a_query_in_a_t_string_is_not_linted() {
    assert_eq!(lint("q = t\"--sql\\nselect distinct(a) from {t}\"\n"), []);
}

#[test]
fn every_dialect_is_one_sqruff_knows() {
    let source = "q = \"\"\"--sql\nselect a,b from t\"\"\"\n";
    for dialect in sqlty_sql::Dialect::NAMES {
        let formatted = format_source(source, dialect, &style());
        assert_ne!(formatted, source, "{dialect} is not laid out");
    }
}

#[test]
fn a_finding_on_a_placeholder_is_not_reported() {
    let source = "q = \"--sql\\nselect distinct(%s), distinct(a) from t\"\n";
    assert_eq!(lint(source), [("AL03", "distinct(a)")]);
}

#[test]
fn blank_lines_stay_blank_and_an_empty_query_stays() {
    assert_eq!(
        format("q = \"\"\"--sql\n  select 1\n\n  from t where b=%s\"\"\"\n"),
        "q = \"\"\"--sql\n  select 1\n\n  from t\n  where b = %s\"\"\"\n"
    );
    let empty = "q = \"\"\"--sql\n   \"\"\"\n";
    assert_eq!(format(empty), empty);
}

#[test]
fn a_query_of_concatenated_literals_is_not_formatted() {
    let source = "q = (\"--sql\\nselect a \" \"from t where b=%s\")\n";
    assert_eq!(format(source), source);
}

#[test]
fn a_query_past_max_tokens_is_neither_formatted_nor_linted() {
    // The limit is 4096 tokens: a word or a punctuation mark each. Only the size matters
    // here, so the query is a sum of `a`s.
    const MAX_TOKENS: usize = 4096;
    let sum = |terms: usize| vec!["a"; terms].join("+");
    let short = format!("q = \"\"\"--sql\nselect {}\"\"\"\n", sum(2));
    assert_ne!(format(&short), short);
    // `select`, each `a` and each `+`: one past the limit.
    let long = format!(
        "q = \"\"\"--sql\nselect {}\"\"\"\n",
        sum(MAX_TOKENS / 2 + 1)
    );
    assert_eq!(format(&long), long);
    let distinct = format!(
        "q = \"--sql\\nselect distinct(a), {} from t\"\n",
        sum(MAX_TOKENS / 2)
    );
    assert_eq!(lint(&distinct), []);
}

#[test]
fn a_query_sqruff_panics_on_is_not_formatted() {
    let source = format!(
        "q = \"\"\"--sql\nSELECT * FROM t AS x({}--c\n,)\"\"\"\n",
        "a".repeat(80)
    );
    assert_eq!(format(&source), source);
}

#[test]
fn a_query_whose_fix_moves_more_than_whitespace_stays() {
    let placeholder = "q = \"\"\"--sql\nselect __sqlty_0__,%s from t\"\"\"\n";
    assert_eq!(format(placeholder), placeholder);
    let comment = "q = \"\"\"--sql\nselect a -- c\n, b from t\"\"\"\n";
    assert_eq!(format(comment), comment);
}
