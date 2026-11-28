//! `highlight`: what names resolve to against a schema, which words are functions, and what stays uncolored.

use sqlty_sql::analyzer::load_schema_with_skipped;
use sqlty_sql::highlight::{highlight, Highlight};

fn names<'a>(sql: &'a str, schema: &str, dialect: &str) -> Vec<(&'a str, Highlight)> {
    let (schema, _) = load_schema_with_skipped(schema, dialect);
    highlight(sql, dialect, Some(&schema))
        .into_iter()
        .filter(|(_, kind)| {
            !matches!(
                kind,
                Highlight::Keyword | Highlight::Operator | Highlight::Number | Highlight::String
            )
        })
        .map(|(range, kind)| (&sql[range], kind))
        .collect()
}

#[test]
fn quoted_names_keep_their_quotes() {
    let schema = r#"CREATE TABLE "Users" ("Id" INT); CREATE TABLE app.t (x INT);"#;
    assert_eq!(
        names(
            r#"SELECT "U"."Id", app.t.x FROM "Users" AS "U", app.t"#,
            schema,
            "postgres"
        ),
        [
            (r#""U""#, Highlight::Variable { declaration: false }),
            (r#""Id""#, Highlight::Column { deprecated: false }),
            ("app", Highlight::Schema),
            ("t", Highlight::Table),
            ("x", Highlight::Column { deprecated: false }),
            (r#""Users""#, Highlight::Table),
            (r#""U""#, Highlight::Variable { declaration: true }),
            ("app", Highlight::Schema),
            ("t", Highlight::Table),
        ]
    );
}

#[test]
fn a_table_read_by_its_name_is_the_table() {
    let schema = "CREATE TABLE Users (id INT)";
    assert_eq!(
        names("SELECT USERS.id FROM Users", schema, "postgres"),
        [
            ("USERS", Highlight::Table),
            ("id", Highlight::Column { deprecated: false }),
            ("Users", Highlight::Table),
        ]
    );
}

#[test]
fn cast_types() {
    let schema = "CREATE DOMAIN email AS TEXT; CREATE TABLE t (x TEXT)";
    let types: Vec<_> = names(
        "SELECT x::email, CAST(x AS int), x::pg_catalog.int4, x::varchar(3), \
         coalesce(x, 'a') AS int FROM t",
        schema,
        "postgres",
    )
    .into_iter()
    .filter(|(_, kind)| matches!(kind, Highlight::Type { .. }))
    .collect();
    assert_eq!(
        types,
        [
            ("email", Highlight::Type { builtin: false }),
            ("int", Highlight::Type { builtin: true }),
            ("int4", Highlight::Type { builtin: true }),
            ("varchar", Highlight::Type { builtin: true }),
        ]
    );
}

#[test]
fn names_the_schema_does_not_define_stay_uncolored() {
    let schema = "CREATE TABLE users (id INT)";
    assert_eq!(
        names("SELECT id FROM users", schema, "postgres"),
        [
            ("id", Highlight::Column { deprecated: false }),
            ("users", Highlight::Table),
        ]
    );
    assert_eq!(names("SELECT id FROM users", "", "postgres"), []);
}

#[test]
fn many_names_in_one_long_line() {
    let schema = "CREATE TABLE t (id INT)";
    let sql = format!("SELECT {} FROM t", vec!["t.id"; 2_000].join(", "));
    assert_eq!(names(&sql, schema, "postgres").len(), 2 * 2_000 + 1);
}

fn functions<'a>(sql: &'a str, dialect: &str) -> Vec<&'a str> {
    highlight(sql, dialect, None)
        .into_iter()
        .filter(|(_, kind)| matches!(kind, Highlight::Function { .. }))
        .map(|(range, _)| &sql[range])
        .collect()
}

#[test]
fn a_word_called_is_a_function() {
    for dialect in ["postgres", "mysql", "sqlite", "duckdb"] {
        assert_eq!(
            functions(
                "SELECT count(*), coalesce(name, ''), count FROM t WHERE created < now()",
                dialect
            ),
            ["count", "coalesce", "now"],
            "{dialect}"
        );
    }
    assert_eq!(
        functions(
            "SELECT pg_catalog.now(), \"my fn\"(1) FROM t WHERE id IN (1) AND EXISTS (SELECT 1)",
            "postgres"
        ),
        ["now", "\"my fn\""]
    );
    assert_eq!(
        functions(
            "WITH c(a) AS (SELECT 1) INSERT INTO public.t(a) SELECT CAST(a AS numeric(10, 2)), \
             a::varchar(3), count (a) FROM c AS x(a) JOIN u USING (a) VALUES (1)",
            "postgres"
        ),
        Vec::<&str>::new()
    );
    for sql in [
        "WITH a AS (SELECT 1), b(x) AS (SELECT 2) SELECT x FROM b",
        "SELECT n FROM generate_series(1, 3) g(n)",
        "SELECT id FROM unnest(%(ids)s::int[]) t(id)",
        "CREATE TABLE IF NOT EXISTS t(a varchar(3), b numeric(10, 2))",
        "COPY users(id) FROM STDIN",
        "INSERT INTO /* c */ users(id) VALUES (1)",
    ] {
        assert!(
            functions(sql, "postgres")
                .iter()
                .all(|name| ["generate_series", "unnest"].contains(name)),
            "{sql}"
        );
    }
    assert_eq!(
        functions("SELECT count(*) AS n, lower(x) FROM t", "postgres"),
        ["count", "lower"]
    );
}
