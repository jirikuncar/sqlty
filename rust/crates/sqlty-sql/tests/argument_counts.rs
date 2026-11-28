//! `argument-count-mismatch`: calls of built-in functions that none of their overloads take.

use sqlty_sql::analyzer::{load_schema_with_skipped, parse};
use sqlty_sql::diagnostics::Rule;

const SCHEMA: &str = "CREATE TABLE users (id INTEGER, name TEXT);
    CREATE FUNCTION lpad(n INTEGER) RETURNS TEXT LANGUAGE sql AS $$ SELECT n::text $$;";

fn reported(sql: &str, dialect: &str) -> Vec<(String, Option<String>)> {
    let schema = load_schema_with_skipped(SCHEMA, dialect).0;
    parse(sql, dialect)
        .analyze_with_diagnostics(Some(&schema))
        .1
        .into_iter()
        .filter(|found| found.rule == Rule::ArgumentCountMismatch)
        .map(|found| (found.message, found.span.map(|span| sql[span].to_string())))
        .collect()
}

fn message(sql: &str, dialect: &str) -> String {
    reported(sql, dialect)
        .into_iter()
        .map(|(message, _)| message)
        .collect()
}

#[test]
fn a_call_no_overload_takes_is_reported_at_its_name() {
    assert_eq!(
        reported("SELECT id FROM users WHERE lower(name, 1) = ''", "postgres"),
        [(
            "`lower` takes 1 argument, got 2: lower(text) -> text; lower(anyrange) -> \
             anyelement; lower(anymultirange) -> anyelement"
                .to_string(),
            Some("lower".to_string())
        )]
    );
    assert_eq!(
        message("SELECT concat() FROM users", "postgres"),
        "`concat` takes at least 1 argument, got 0: concat(val1: \"any\"[, val2: \"any\", ...]) -> text"
    );
    assert!(
        message("SELECT ltrim(name, 'a', 'b') FROM users", "postgres")
            .starts_with("`ltrim` takes 1 or 2 arguments, got 3")
    );
    assert!(message("SELECT json_serialize_sql()", "duckdb")
        .starts_with("`json_serialize_sql` takes 1 to 5 arguments, got 0"));
    assert_eq!(
        message("SELECT LEFT(name) FROM users", "mysql"),
        "`left` takes 2 arguments, got 1: left(str, len)"
    );
}

#[test]
fn a_star_is_an_argument_of_count_only() {
    assert!(message("SELECT upper(*) FROM users", "postgres")
        .starts_with("`upper` takes no `*`, only `count` does: upper(text) -> text"));
    assert!(reported("SELECT count(*) FROM users", "postgres").is_empty());
}

#[test]
fn calls_whose_arguments_are_not_counted_are_not_reported() {
    for sql in [
        "SELECT ltrim(name), ltrim(name, 'x'), coalesce(id, id, id) FROM users",
        "SELECT lpad(id) FROM users",
        "SELECT make_interval(days => 1)",
        "SELECT rank(1) WITHIN GROUP (ORDER BY id) FROM users",
        "SELECT no_such_function(1, 2) FROM users",
        "SELECT current_date",
    ] {
        assert!(reported(sql, "postgres").is_empty(), "{sql}");
    }
    assert!(reported("SELECT lower(name, 1) FROM users", "sqlite").is_empty());
}
