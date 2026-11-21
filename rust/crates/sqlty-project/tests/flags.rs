//! The command line's `-c` assignments and how a string is spelled in its messages.

use sqlty_project::{parse_overrides, repr};
use toml::Table;

fn overrides(assignments: &[&str]) -> Result<Table, String> {
    let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
    parse_overrides(&assignments)
}

#[test]
fn merges_the_assignments_key_by_key() {
    let table = overrides(&[
        "rules.select-star = \"warn\"",
        "rules = { unknown-column = \"ignore\" }",
        "dialect = \"mysql\"",
        "dialect = \"sqlite\"",
    ])
    .expect("parses");
    assert_eq!(
        table.to_string(),
        "dialect = \"sqlite\"\n\n[rules]\nselect-star = \"warn\"\nunknown-column = \"ignore\"\n"
    );
}

#[test]
fn reports_what_is_not_an_assignment() {
    assert_eq!(
        overrides(&["ci.toml"]).err().as_deref(),
        Some(
            "--config expects a TOML assignment such as 'dialect = \"mysql\"', got 'ci.toml'; \
             use --config-file for a file"
        )
    );
    let error = overrides(&["dialect = mysql"]).err().unwrap_or_default();
    assert!(
        error.starts_with("--config 'dialect = mysql': ")
            && error.ends_with(
                "; quote strings, as in -c 'dialect = \"mysql\"', or use --config-file for a file"
            ),
        "{error}"
    );
    let error = overrides(&["dialet = \"mysql\""]).err().unwrap_or_default();
    assert!(error.contains("dialect"), "{error}");
}

#[test]
fn spells_strings_as_python_does() {
    assert_eq!(repr("it's"), "\"it's\"");
    assert_eq!(repr("a'\"b"), "'a\\'\"b'");
    assert_eq!(repr("\\\n\r\t\u{7}é"), "'\\\\\\n\\r\\t\\x07é'");
}
