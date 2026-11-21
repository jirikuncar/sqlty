use crate::support::*;
use sqlty_sql::analyzer::*;

/// The replacement `unknown-column` offers for `name` over a table `t` with `columns`
/// (and a table `u` with the same ones, when `twice`).
fn suggestion(name: &str, columns: &[&str], twice: bool) -> Option<String> {
    let table = columns
        .iter()
        .map(|column| format!("{column} INT"))
        .collect::<Vec<_>>()
        .join(", ");
    let schema = load_schema(
        &format!("CREATE TABLE t ({table}); CREATE TABLE u ({table});"),
        "postgres",
    );
    let from = if twice { "t, u" } else { "t" };
    let (_, diagnostics) = analyze_with_diagnostics(
        &format!("SELECT {name} FROM {from}"),
        "postgres",
        Some(&schema),
    );
    let unknown = diagnostics
        .iter()
        .find(|d| d.rule.name() == "unknown-column")
        .unwrap_or_else(|| panic!("no unknown-column for {name}: {diagnostics:?}"));
    unknown.replacement.clone()
}

#[test]
fn suggests_the_one_close_name() {
    assert_eq!(
        suggestion("surname", &["id", "name", "email"], false).as_deref(),
        Some("name")
    );
    assert_eq!(
        suggestion("nmae", &["id", "name"], false).as_deref(),
        Some("name")
    );
    assert_eq!(suggestion("NAM", &["name"], false).as_deref(), Some("name"));
    assert_eq!(
        suggestion("abcdefgh", &["abcdefgx", "abcdefxx"], false).as_deref(),
        Some("abcdefgx")
    );
}

#[test]
fn suggests_nothing_far_or_tied() {
    assert_eq!(suggestion("email", &["id", "name"], false), None);
    assert_eq!(suggestion("ab", &["xy"], false), None);
    assert_eq!(suggestion("xyid", &["id"], false), None);
    assert_eq!(suggestion("ip", &["id"], false), None);
    assert_eq!(suggestion("coxt", &["cost", "coat"], false), None);
    assert_eq!(suggestion("nme", &["name"], true), None);
}

#[test]
fn a_name_inside_another_is_close_when_it_has_four_characters() {
    assert_eq!(
        suggestion("username", &["name", "email"], false).as_deref(),
        Some("name")
    );
    assert_eq!(suggestion("usr", &["id", "usr_name"], false), None);
}
