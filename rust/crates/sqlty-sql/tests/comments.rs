//! What the schema's `COMMENT`s say of tables and columns, shown by hover and carried by
//! completion; `tests/mdtest/hover/comments.md` has the cases in full.

use sqlty_sql::analyzer::{complete, hover, load_schema_with_skipped};

const SCHEMA: &str = "CREATE TABLE users (id INT PRIMARY KEY, name TEXT);
COMMENT ON TABLE users IS 'People who sign in';
COMMENT ON COLUMN users.name IS 'Shown on the profile';
CREATE TABLE orders (id INT, user_id INT REFERENCES users (id));";

/// `(label, description)` of what completion offers at the end of `sql`.
fn described(sql: &str) -> Vec<(String, Option<String>)> {
    let (schema, _) = load_schema_with_skipped(SCHEMA, "postgres");
    let cursor = sql.find('|').unwrap_or(sql.len());
    let sql = sql.replacen('|', "", 1);
    complete(&sql, cursor, "postgres", Some(&schema), 10)
        .candidates
        .into_iter()
        .map(|c| (c.label, c.description))
        .collect()
}

#[test]
fn completion_carries_the_comment() {
    let shown = |label: &str, text: Option<&str>| (label.to_string(), text.map(str::to_string));
    assert_eq!(
        described("SELECT na| FROM users"),
        [shown("name", Some("Shown on the profile"))]
    );
    assert_eq!(
        described("SELECT 1 FROM us"),
        [shown("users", Some("People who sign in"))]
    );
    assert_eq!(
        described("SELECT 1 FROM orders o JOIN us"),
        [
            shown("users ON users.id = o.user_id", Some("People who sign in")),
            shown("users", Some("People who sign in")),
        ]
    );
    assert_eq!(described("SELECT 1 FROM ord"), [shown("orders", None)]);
}

/// What hover shows at the `|` of `sql` for a schema of `dialect`, `None` for nothing.
fn hovered(schema_sql: &str, dialect: &str, sql: &str) -> Option<String> {
    let (schema, _) = load_schema_with_skipped(schema_sql, dialect);
    let cursor = sql.find('|')?;
    let sql = sql.replacen('|', "", 1);
    hover(&sql, cursor, dialect, Some(&schema)).map(|found| found.contents)
}

const POSTGRES: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
COMMENT ON TABLE users IS 'People who sign in';
COMMENT ON COLUMN users.name IS 'Shown on the profile';
COMMENT ON COLUMN users.email IS 'Login';
COMMENT ON COLUMN users.email IS NULL;
ALTER TABLE users RENAME COLUMN name TO full_name;
CREATE VIEW names AS SELECT full_name FROM users;
COMMENT ON VIEW names IS 'Public names';
COMMENT ON COLUMN names.full_name IS 'As shown';
CREATE TABLE gone (id INT);
COMMENT ON TABLE gone IS 'Dropped';
DROP TABLE gone;
CREATE TABLE gone (id INT);
CREATE TABLE old (id INT);
COMMENT ON COLUMN old.id IS 'Kept';
COMMENT ON TABLE old IS '';
ALTER TABLE old RENAME TO kept;
ALTER TABLE kept RENAME COLUMN id TO key;
ALTER TABLE kept ALTER COLUMN key TYPE BIGINT;
CREATE INDEX kept_key ON kept (key);
COMMENT ON INDEX kept_key IS 'Index';
COMMENT ON TABLE nowhere IS 'None';
COMMENT ON COLUMN kept.nope IS 'None';
COMMENT ON TABLE pg_class IS 'Catalog';
COMMENT ON TABLE users IS 1;
COMMENT ON COLUMN pg_class.relname IS 'Catalog';
CREATE VIEW v AS SELECT id FROM users;
COMMENT ON COLUMN v.id IS 'Gone';
COMMENT ON COLUMN v.id IS NULL;";

#[test]
fn hover_shows_the_comment_in_postgres() {
    let pg = |sql: &str| hovered(POSTGRES, "postgres", sql).unwrap_or_default();
    assert_eq!(
        pg("SELECT id FROM users WHERE full_na|me = ''"),
        "Column `full_name` of `users`: `TEXT NULL`\n\nShown on the profile"
    );
    assert_eq!(
        pg("SELECT full_na|me FROM users"),
        "Result column `full_name`: `str | None` (`TEXT`)\n\nShown on the profile"
    );
    assert!(pg("SELECT 1 FROM use|rs").starts_with("Table `public.users`\n\nPeople who sign in\n"));
    assert_eq!(
        pg("SELECT id FROM users WHERE ema|il = ''"),
        "Column `email` of `users`: `TEXT NULL`"
    );
    assert!(!pg("SELECT LENGTH(full_name) || full_na|me AS x FROM users").contains("profile"));
    assert_eq!(
        pg("SELECT 1 FROM names n WHERE n.full_na|me = ''"),
        "Column `full_name` of `n`: `TEXT NULL`\n\nAs shown"
    );
    assert!(pg("SELECT 1 FROM na|mes").starts_with("View `public.names`\n\nPublic names\n"));
    assert_eq!(
        pg("SELECT 1 FROM go|ne"),
        "Table `public.gone`\n- `id` `INT NULL`"
    );
    assert_eq!(
        pg("SELECT 1 FROM kept WHERE ke|y = 1"),
        "Column `key` of `kept`: `BIGINT NULL`\n\nKept"
    );
    assert_eq!(
        pg("SELECT 1 FROM ke|pt"),
        "Table `public.kept`\n- `key` `BIGINT NULL`"
    );
    assert_eq!(
        pg("SELECT 1 FROM v WHERE i|d = 1"),
        "Column `id` of `v`: `INT NOT NULL`"
    );
}

#[test]
fn hover_shows_the_comment_in_mysql_and_duckdb() {
    const MYSQL: &str = "CREATE TABLE users (id INT PRIMARY KEY COMMENT 'Key', name TEXT, email TEXT COMMENT 'Old') COMMENT = 'Before';
ALTER TABLE users COMMENT = 'People';
ALTER TABLE users MODIFY COLUMN name TEXT COMMENT 'Full name';
ALTER TABLE users MODIFY COLUMN email TEXT;
CREATE TABLE t (id INT) COMMENT 'It''s \"t\"';
ALTER TABLE t COMMENT;
CREATE TABLE u (id INT) COMMENT 'Before';
ALTER TABLE u COMMENT = 'It\\'s u';";
    let my = |sql: &str| hovered(MYSQL, "mysql", sql).unwrap_or_default();
    assert_eq!(
        my("SELECT name FROM users WHERE i|d = 1"),
        "Column `id` of `users`: `INT NOT NULL`\n\nKey"
    );
    assert!(my("SELECT 1 FROM use|rs").starts_with("Table `users`\n\nPeople\n"));
    assert_eq!(
        my("SELECT id FROM users WHERE na|me = ''"),
        "Column `name` of `users`: `TEXT NULL`\n\nFull name"
    );
    assert_eq!(
        my("SELECT id FROM users WHERE ema|il = ''"),
        "Column `email` of `users`: `TEXT NULL`"
    );
    assert!(my("SELECT 1 FROM |t").starts_with("Table `t`\n\nIt's \"t\"\n"));
    assert!(my("SELECT 1 FROM |u").starts_with("Table `u`\n\nIt's u\n"));
    let duck = "CREATE TABLE t (a INT);\nCOMMENT ON COLUMN t.a IS 'Counted';";
    assert_eq!(
        hovered(duck, "duckdb", "SELECT 1 FROM t WHERE |a = 1").unwrap_or_default(),
        "Column `a` of `t`: `INT NULL`\n\nCounted"
    );
}
