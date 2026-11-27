//! What the schema's `COMMENT`s say of tables, views and columns, in hover and completion.

use sqlty_sql::analyzer::{complete, hover, load_schema_with_skipped};

/// The hover of the word at `|` of `sql`, in a schema of `schema_sql`.
fn at(schema_sql: &str, dialect: &str, sql: &str) -> Option<String> {
    let cursor = sql.find('|')?;
    let sql = sql.replacen('|', "", 1);
    let (schema, _) = load_schema_with_skipped(schema_sql, dialect);
    Some(hover(&sql, cursor, dialect, Some(&schema))?.contents)
}

const PG: &str = "CREATE TABLE users (id INT, name TEXT, email TEXT);
COMMENT ON TABLE users IS 'People who sign in';
COMMENT ON COLUMN users.name IS 'Shown on the profile';
COMMENT ON COLUMN users.email IS 'Old';
COMMENT ON COLUMN users.email IS NULL;
CREATE VIEW names AS SELECT name FROM users;
COMMENT ON VIEW names IS 'Public names';
COMMENT ON COLUMN names.name IS 'As shown';
CREATE TABLE gone (id INT);
COMMENT ON TABLE gone IS 'Dropped';
DROP TABLE gone;
CREATE TABLE gone (id INT);
CREATE TABLE old (id INT);
COMMENT ON COLUMN old.id IS 'Kept';
ALTER TABLE old RENAME TO kept;
ALTER TABLE kept RENAME COLUMN id TO key;
ALTER TABLE kept ALTER COLUMN key TYPE BIGINT;";

fn pg(sql: &str) -> Option<String> {
    at(PG, "postgres", sql)
}

#[test]
fn hover_shows_the_comment_under_the_type() {
    assert_eq!(
        pg("SELECT na|me FROM users").as_deref(),
        Some("Result column `name`: `str | None` (`TEXT`)\n\nShown on the profile")
    );
    assert_eq!(
        pg("SELECT 1 FROM users WHERE na|me = ''").as_deref(),
        Some("Column `name` of `users`: `TEXT NULL`\n\nShown on the profile")
    );
    assert_eq!(
        pg("SELECT 1 FROM use|rs").as_deref(),
        Some(
            "Table `public.users`\n\nPeople who sign in\n\n- `id` `INT NULL`\n- `name` `TEXT NULL`\n- `email` `TEXT NULL`"
        )
    );
    assert_eq!(
        pg("SELECT 1 FROM users WHERE ema|il = ''").as_deref(),
        Some("Column `email` of `users`: `TEXT NULL`")
    );
    assert_eq!(
        pg("SELECT 1 FROM names n WHERE n.na|me = ''").as_deref(),
        Some("Column `name` of `n`: `TEXT NULL`\n\nAs shown")
    );
    assert!(pg("SELECT 1 FROM nam|es").is_some_and(|text| text.contains("\n\nPublic names\n")));
    assert!(pg("SELECT 1 FROM go|ne").is_some_and(|text| !text.contains("Dropped")));
    assert_eq!(
        pg("SELECT 1 FROM kept WHERE k|ey = 1").as_deref(),
        Some("Column `key` of `kept`: `BIGINT NULL`\n\nKept")
    );
}

#[test]
fn completion_carries_the_comment() {
    let (schema, _) = load_schema_with_skipped(PG, "postgres");
    let sql = "SELECT na FROM users";
    let found = complete(sql, 9, "postgres", Some(&schema), 10);
    let described: Vec<_> = found
        .candidates
        .iter()
        .map(|c| (c.label.as_str(), c.description.as_deref()))
        .collect();
    assert_eq!(described, [("name", Some("Shown on the profile"))]);
    let found = complete("SELECT 1 FROM us", 16, "postgres", Some(&schema), 10);
    assert_eq!(
        found.candidates[0].description.as_deref(),
        Some("People who sign in")
    );
}

#[test]
fn mysql_reads_inline_comments() {
    let schema = "CREATE TABLE users (id INT COMMENT 'Key', name TEXT) COMMENT = 'People';
ALTER TABLE users MODIFY COLUMN name TEXT COMMENT 'Full name';";
    assert_eq!(
        at(schema, "mysql", "SELECT 1 FROM users WHERE i|d = 1").as_deref(),
        Some("Column `id` of `users`: `INT NULL`\n\nKey")
    );
    assert_eq!(
        at(schema, "mysql", "SELECT 1 FROM users WHERE na|me = ''").as_deref(),
        Some("Column `name` of `users`: `TEXT NULL`\n\nFull name")
    );
    assert!(at(schema, "mysql", "SELECT 1 FROM use|rs").is_some_and(|t| t.contains("\n\nPeople\n")));
}

#[test]
fn duckdb_reads_comment_on() {
    let schema = "CREATE TABLE t (a INT);\nCOMMENT ON COLUMN t.a IS 'Counted';";
    assert_eq!(
        at(schema, "duckdb", "SELECT 1 FROM t WHERE |a = 1").as_deref(),
        Some("Column `a` of `t`: `INT NULL`\n\nCounted")
    );
}
