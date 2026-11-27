//! What the schema's `COMMENT`s say of tables and columns, carried by completion. Hover
//! shows them too: `tests/mdtest/hover/comments.md`.

use sqlty_sql::analyzer::{complete, load_schema_with_skipped};

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
