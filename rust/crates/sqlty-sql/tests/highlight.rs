//! `highlight`: the colored tokens of a query.

#![expect(
    clippy::expect_used,
    reason = "a test fails on the query it cannot read"
)]

use sqlty_sql::highlight::{highlight, Highlight};

fn colored<'a>(sql: &'a str, dialect: &str) -> Vec<(&'a str, Highlight)> {
    highlight(sql, dialect)
        .into_iter()
        .map(|(range, kind)| (&sql[range], kind))
        .collect()
}

#[test]
fn a_long_line_with_many_placeholders() {
    // Each token used to count characters from its line's start and look at every
    // placeholder: quadratic in the line's length.
    let sql = format!(
        "SELECT x\nFROM t WHERE a IN ({})",
        vec!["%s"; 50_000].join(", ")
    );
    let found = colored(&sql, "postgres");
    assert_eq!(found.len(), 4 + 50_000);
    assert_eq!(
        found[3..6],
        [
            ("IN", Highlight::Keyword),
            ("%s", Highlight::Parameter),
            ("%s", Highlight::Parameter)
        ]
    );
}

#[test]
fn names_that_sqlparser_knows_as_keywords_stay_names() {
    let sql = "SELECT name, status, \"select\", t.order FROM t WHERE type IS NOT NULL";
    let keywords: Vec<_> = colored(sql, "postgres")
        .into_iter()
        .filter(|(_, kind)| *kind == Highlight::Keyword)
        .map(|(text, _)| text)
        .collect();
    assert_eq!(keywords, ["SELECT", "FROM", "WHERE", "IS", "NOT", "NULL"]);
}

#[test]
fn literals_comments_and_operators() {
    assert_eq!(
        colored(
            "SELECT 'a''b', 1.5, data->>'k', x::int /* c\n d */ FROM t;",
            "postgres"
        ),
        [
            ("SELECT", Highlight::Keyword),
            ("'a''b'", Highlight::String),
            ("1.5", Highlight::Number),
            ("->>", Highlight::Operator),
            ("'k'", Highlight::String),
            ("::", Highlight::Operator),
            ("/* c\n d */", Highlight::Comment),
            ("FROM", Highlight::Keyword),
        ]
    );
    assert_eq!(
        colored("SELECT %(id)s % 2, '%s' FROM t", "postgres"),
        [
            ("SELECT", Highlight::Keyword),
            ("%(id)s", Highlight::Parameter),
            ("%", Highlight::Operator),
            ("2", Highlight::Number),
            ("'%s'", Highlight::String),
            ("FROM", Highlight::Keyword),
        ]
    );
    assert_eq!(
        colored("SELECT ? -- é\n", "sqlite"),
        [
            ("SELECT", Highlight::Keyword),
            ("?", Highlight::Parameter),
            ("-- é", Highlight::Comment),
        ]
    );
}

#[test]
fn an_unterminated_token_ends_the_colors() {
    assert_eq!(
        colored("SELECT 1 FROM t WHERE x = 'ab", "postgres"),
        [
            ("SELECT", Highlight::Keyword),
            ("1", Highlight::Number),
            ("FROM", Highlight::Keyword),
            ("WHERE", Highlight::Keyword),
            ("=", Highlight::Operator),
        ]
    );
}

/// The words the VS Code grammar colors as keywords, `ALL` first.
fn grammar_keywords(grammar: &str) -> Vec<&str> {
    let (_, rest) = grammar
        .split_once("(?:ALL|")
        .expect("the grammar lists keywords");
    let (words, _) = rest.split_once(')').expect("the list ends");
    std::iter::once("ALL").chain(words.split('|')).collect()
}

#[test]
fn the_vs_code_grammars_color_the_keywords_highlight_colors() {
    let grammars = [
        include_str!("../../../../editors/vscode/syntaxes/sql-injection.tmLanguage.json"),
        include_str!("../../../../editors/vscode/syntaxes/sql-injection-multi.tmLanguage.json"),
    ];
    // Every word sqlparser knows, and the ones the grammar lists: a word is colored when it
    // is listed, so each listed word is found (the list is sorted for a binary search) and
    // no other is.
    let colored: Vec<&str> = sqlparser::keywords::ALL_KEYWORDS
        .iter()
        .copied()
        .chain(grammar_keywords(grammars[0]))
        .filter(|word| {
            colored(word, "postgres")
                .iter()
                .any(|(text, kind)| *kind == Highlight::Keyword && text == word)
        })
        .collect();
    for grammar in grammars {
        let mut listed = grammar_keywords(grammar);
        let mut colored = colored.clone();
        listed.sort_unstable();
        colored.sort_unstable();
        colored.dedup();
        assert_eq!(colored, listed);
    }
}
