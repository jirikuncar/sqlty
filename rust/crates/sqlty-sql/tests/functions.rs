//! Function names in completion and hover: the schema's functions and the dialect's built-ins.

use sqlty_sql::analyzer::{complete, hover, load_schema_with_skipped, CandidateKind};

const SCHEMA: &str = "CREATE TABLE users (id INTEGER, name TEXT, email TEXT);
    CREATE FUNCTION normalize_email(email TEXT) RETURNS TEXT LANGUAGE sql AS $$ SELECT $1 $$;
    CREATE FUNCTION lower(x INTEGER) RETURNS INTEGER LANGUAGE sql AS $$ SELECT x $$;";

fn completed(sql: &str, dialect: &str) -> Vec<(String, CandidateKind, Option<String>)> {
    let cursor = sql.find('|').unwrap_or(sql.len());
    let sql = sql.replacen('|', "", 1);
    let schema = load_schema_with_skipped(SCHEMA, dialect).0;
    complete(&sql, cursor, dialect, Some(&schema), 4)
        .candidates
        .into_iter()
        .map(|c| (c.label, c.kind, c.detail))
        .collect()
}

fn hovered(sql: &str) -> Option<String> {
    let cursor = sql.find('|').unwrap_or(sql.len());
    let sql = sql.replacen('|', "", 1);
    let schema = load_schema_with_skipped(SCHEMA, "postgres").0;
    hover(&sql, cursor, "postgres", Some(&schema)).map(|found| found.contents)
}

fn function(label: &str, detail: &str) -> (String, CandidateKind, Option<String>) {
    (
        label.to_string(),
        CandidateKind::Function,
        Some(detail.to_string()),
    )
}

#[test]
fn functions_complete_after_columns() {
    assert_eq!(
        completed("SELECT nor| FROM users", "postgres"),
        [function("normalize_email", "public")]
    );
    let found = completed("SELECT n FROM users WHERE n|", "postgres");
    assert_eq!(found[0].1, CandidateKind::Column);
    assert!(
        found[1..].iter().all(|c| c.1 == CandidateKind::Function),
        "{found:?}"
    );
    assert_eq!(
        completed("SELECT UPPE|", "mysql"),
        [function("upper", "upper(str)")]
    );
    assert_eq!(
        completed("SELECT ltri|", "postgres")[0].2.as_deref(),
        Some("ltrim(string: text[, characters: text]) -> text (+1 more)")
    );
    assert_eq!(
        completed("SELECT low| FROM users", "postgres")
            .iter()
            .map(|c| c.2.as_deref().unwrap_or_default())
            .take(2)
            .collect::<Vec<_>>(),
        ["public", "lower(text) -> text (+2 more)"],
        "the schema's function first"
    );
    assert!(completed("SELECT u.low| FROM users u", "postgres").is_empty());
    assert!(completed("SELECT | FROM users", "postgres")
        .iter()
        .all(|c| c.1 == CandidateKind::Column));
}

#[test]
fn a_called_function_hovers_its_fitting_overloads() {
    let one = "```sql\nltrim(string: text[, characters: text]) -> text\n```\n\n\
        [Documentation](https://www.postgresql.org/docs/17/functions-string.html#FUNCTIONS-STRING-SQL)";
    assert_eq!(
        hovered("SELECT 1 FROM users WHERE lt|rim(name) = ''").as_deref(),
        Some(one)
    );
    assert_eq!(
        hovered("SELECT 1 FROM users WHERE lt|rim(name, ARRAY[1, 2][1]) = ''")
            .map(|text| text.lines().count()),
        Some(6)
    );
    assert_eq!(
        hovered("SELECT 1 FROM users WHERE lt|rim() = ''").map(|text| text.lines().count()),
        Some(6),
        "no overload fits: all of them"
    );
    assert!(hovered("SELECT 1 FROM users WHERE lt|rim(name").is_some());
    assert_eq!(
        hovered("SELECT id FROM users WHERE normalize_|email(email) = ''").as_deref(),
        Some("Function `normalize_email` the schema defines")
    );
    assert_eq!(
        hovered("SELECT id FROM users WHERE NORMALIZE_|EMAIL(email) = ''").as_deref(),
        Some("Function `normalize_email` the schema defines")
    );
    assert!(hovered("SELECT id FROM users WHERE \"NORMALIZE_|EMAIL\"(email) = ''").is_none());
    assert_eq!(
        hovered("SELECT 1 FROM users WHERE lt|rim(name ORDER BY id, name) = ''").as_deref(),
        Some(one),
        "ORDER BY ends the arguments"
    );
    let both = hovered("SELECT id FROM users WHERE low|er(id) = 1").unwrap_or_default();
    assert!(
        both.starts_with("Function `lower` the schema defines\n\nBuilt-in:\n\n```sql\nlower(text)"),
        "{both}"
    );
}

#[test]
fn a_function_name_hovers_outside_a_call_too() {
    assert!(hovered("SELECT 1 FROM users WHERE coalesc|e IS NULL")
        .is_some_and(|text| text.contains("coalesce(")));
    assert!(hovered("SELECT 1 FROM users WHERE no_such_functio|n(1)").is_none());
    let item = hovered("SELECT LEN|GTH(name) FROM users").unwrap_or_default();
    assert!(
        item.starts_with("Result column `length`") && item.contains("---\n\n```sql\nlength(text)"),
        "{item}"
    );
    assert!(hovered("SELECT up|per(1, 2, 3)").is_some_and(|text| text.contains("upper(text)")));
}

#[test]
fn table_functions_complete_after_relations_in_from() {
    let found = completed("SELECT * FROM users, generate_s|", "postgres");
    assert_eq!(found[0].0, "generate_series");
    assert_eq!(found[0].1, CandidateKind::Function);
    assert!(completed("SELECT * FROM us|", "postgres")[0].1 == CandidateKind::Table);
    assert!(
        completed("SELECT * FROM low|", "postgres").is_empty(),
        "lower returns no rows"
    );
    assert!(completed("UPDATE generate_s|", "postgres").is_empty());
    assert!(completed("SELECT * FROM public.generate_s|", "postgres").is_empty());
    assert!(completed("SELECT * FROM |", "postgres")
        .iter()
        .all(|c| c.1 != CandidateKind::Function));
}
