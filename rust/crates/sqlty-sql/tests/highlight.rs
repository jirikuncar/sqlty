//! `highlight` against a schema: what names resolve to, and what stays uncolored.

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
