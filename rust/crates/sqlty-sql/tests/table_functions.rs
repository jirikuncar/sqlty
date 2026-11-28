//! `unknown-function` for a function in `FROM` that is neither a built-in nor the schema's.

use sqlty_sql::analyzer::{load_schema_with_skipped, parse};
use sqlty_sql::diagnostics::Rule;

const SCHEMA: &str = "CREATE TABLE users (id INTEGER, name TEXT);
    CREATE FUNCTION active_users() RETURNS TABLE (id INTEGER) LANGUAGE sql AS $$ SELECT 1 $$;";

/// `(rule, message, the text it is about, its fix)` of each diagnostic.
fn reported(sql: &str, dialect: &str) -> Vec<(Rule, String, Option<String>, Option<String>)> {
    let schema = load_schema_with_skipped(SCHEMA, dialect).0;
    parse(sql, dialect)
        .analyze_with_diagnostics(Some(&schema))
        .1
        .into_iter()
        .map(|found| {
            let at = found.span.map(|span| sql[span].to_string());
            (found.rule, found.message, at, found.replacement)
        })
        .collect()
}

#[test]
fn a_misspelled_table_function_names_the_one_it_meant() {
    assert_eq!(
        reported("SELECT * FROM generate_seriez(1, 3)", "postgres"),
        [(
            Rule::UnknownFunction,
            "table function `generate_seriez` is not known to sqlty; did you mean \
             `generate_series`?"
                .to_string(),
            Some("generate_seriez".to_string()),
            Some("generate_series".to_string())
        )]
    );
    assert_eq!(
        reported(
            "SELECT e.key FROM users, LATERAL jsonb_each_txt(name::jsonb) e",
            "postgres"
        )[0]
        .3,
        Some("jsonb_each_text".to_string())
    );
}

#[test]
fn only_a_name_within_two_typos_is_suggested() {
    for sql in [
        "SELECT e.key FROM users u, each(u.name::hstore) AS e",
        "SELECT n FROM my_generate_series_helper(1) AS n",
    ] {
        let found = reported(sql, "postgres");
        assert_eq!(found[0].0, Rule::UnknownFunction, "{sql}");
        assert_eq!(found[0].3, None, "{sql}");
    }
    assert_eq!(
        reported(
            "SELECT n FROM pg_catalog.generate_seriez(1, 3) AS n",
            "postgres"
        )[0]
        .3,
        Some("pg_catalog.generate_series".to_string())
    );
}

#[test]
fn a_function_without_a_close_name_is_also_unread() {
    let found: Vec<_> = reported("SELECT * FROM st_dump(NULL)", "postgres")
        .into_iter()
        .map(|(rule, message, _, fix)| (rule, message, fix))
        .collect();
    assert_eq!(
        found,
        [
            (
                Rule::UnknownFunction,
                "table function `st_dump` is not known to sqlty".to_string(),
                None
            ),
            (
                Rule::UnsupportedSyntax,
                "`*` reads table function st_dump, which sqlty does not read the columns of"
                    .to_string(),
                None
            ),
        ]
    );
}

#[test]
fn known_and_unjudged_functions_are_not_reported() {
    for (sql, dialect) in [
        ("SELECT n FROM generate_series(1, 3) AS n", "postgres"),
        (
            "SELECT n FROM pg_catalog.generate_series(1, 3) AS n",
            "postgres",
        ),
        ("SELECT id FROM active_users()", "postgres"),
        ("SELECT x FROM other.rows_of(1) AS r(x)", "postgres"),
        ("SELECT x FROM generate_seriez(1, 3) AS r(x)", "sqlite"),
        ("SELECT x FROM read_csvv('a.csv') AS r(x)", "duckdb"),
    ] {
        assert!(
            reported(sql, dialect)
                .iter()
                .all(|found| found.0 != Rule::UnknownFunction),
            "{sql}"
        );
    }
    let mysql = reported("SELECT x FROM generate_series(1, 3) AS r(x)", "mysql");
    assert_eq!(
        mysql[0].1,
        "table function `generate_series` is not known to sqlty"
    );
}
