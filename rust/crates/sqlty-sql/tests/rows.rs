//! How many rows a statement returns, from its keys, conditions, aggregates and limits.

use sqlty_sql::analyzer::{self, Rows};

const SCHEMA: &str = "
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, team_id INTEGER, name TEXT NOT NULL);
CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE memberships (user_id INTEGER NOT NULL, team_id INTEGER NOT NULL, PRIMARY KEY (user_id, team_id));
CREATE TABLE events (at TIMESTAMP, kind TEXT);
";

use Rows::{AtMostOne, Many, One, Unknown};

#[rustfmt::skip]
const CASES: &[(&str, Rows)] = &[
    ("SELECT 1", One),
    ("SELECT 1 WHERE $1", AtMostOne),
    ("SELECT count(*) FROM users", One),
    ("SELECT count(*) FROM users HAVING count(*) > 1", AtMostOne),
    ("SELECT team_id, count(*) FROM users GROUP BY team_id", Many),
    ("SELECT name FROM users", Many),
    ("SELECT name FROM users WHERE id = $1", AtMostOne),
    ("SELECT name FROM users u WHERE u.email = 'a@b.c'", AtMostOne),
    ("SELECT name FROM users WHERE id = $1 OR id = $2", Many),
    ("SELECT name FROM users WHERE id IN ($1, $2)", Many),
    ("SELECT name FROM users WHERE id > $1", Many),
    ("SELECT name FROM users WHERE team_id = $1", Many),
    ("SELECT name FROM users LIMIT 1", AtMostOne),
    ("SELECT name FROM users FETCH FIRST ROW ONLY", AtMostOne),
    ("SELECT name FROM users LIMIT 2", Many),
    ("SELECT name FROM users ORDER BY id LIMIT 1 OFFSET 3", AtMostOne),
    ("SELECT DISTINCT name FROM users WHERE id = $1", AtMostOne),
    ("SELECT u.name, t.name FROM users u JOIN teams t ON t.id = u.team_id WHERE u.id = $1", AtMostOne),
    ("SELECT u.name, t.name FROM users u LEFT JOIN teams t ON t.id = u.team_id WHERE u.id = $1", AtMostOne),
    ("SELECT u.name FROM users u LEFT JOIN teams t ON u.id = $1 AND t.id = u.team_id", Many),
    ("SELECT u.name FROM users u JOIN teams t USING (id) WHERE t.id = $1", AtMostOne),
    ("SELECT t.name FROM teams t JOIN users u ON u.team_id = t.id WHERE t.id = $1", Many),
    ("SELECT 1 FROM memberships WHERE user_id = $1", Many),
    ("SELECT 1 FROM memberships WHERE user_id = $1 AND team_id = $2", AtMostOne),
    ("SELECT 1 FROM users, teams WHERE users.id = $1 AND teams.id = users.team_id", AtMostOne),
    ("SELECT kind FROM events WHERE at = $1", Many),
    ("SELECT name FROM unknown_table WHERE id = $1", Unknown),
    ("SELECT name FROM users UNION SELECT name FROM teams", Unknown),
    ("WITH u AS (SELECT * FROM users) SELECT name FROM u", Unknown),
    ("SELECT generate_series(1, 3)", Unknown),
    ("SELECT u.name FROM users u RIGHT JOIN teams t ON t.id = u.team_id WHERE u.id = $1", Unknown),
    ("VALUES (1)", One),
    ("VALUES (1), (2)", Many),
    ("INSERT INTO teams (name) VALUES ($1) RETURNING id", One),
    ("INSERT INTO teams DEFAULT VALUES RETURNING id", One),
    ("INSERT INTO teams (name) VALUES ($1), ($2) RETURNING id", Many),
    ("INSERT INTO teams (id, name) VALUES ($1, $2) ON CONFLICT DO NOTHING RETURNING id", AtMostOne),
    ("INSERT INTO teams (id, name) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET name = excluded.name RETURNING id", One),
    ("INSERT INTO teams (name) SELECT name FROM users RETURNING id", Many),
    ("UPDATE users SET name = $1 WHERE id = $2 RETURNING name", AtMostOne),
    ("UPDATE users SET name = $1 RETURNING name", Many),
    ("DELETE FROM users WHERE email = $1 RETURNING id", AtMostOne),
    ("DELETE FROM users WHERE name = $1 RETURNING id", Many),
    ("CREATE TABLE t (a INT)", Unknown),
];

#[test]
fn each_statement_returns_the_rows_its_keys_and_clauses_allow() {
    let (schema, _) = analyzer::load_schema_with_skipped(SCHEMA, "postgres");
    let failures: Vec<String> = CASES
        .iter()
        .filter_map(|&(sql, expected)| {
            let actual = analyzer::parse(sql, "postgres").rows(Some(&schema));
            (actual != expected).then(|| format!("    ({sql:?}, {actual:?}),"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} case(s) differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn sqlite_reads_its_own_conflict_clauses() {
    let (schema, _) = analyzer::load_schema_with_skipped(SCHEMA, "sqlite");
    let rows = |sql: &str| analyzer::parse(sql, "sqlite").rows(Some(&schema));
    assert_eq!(
        rows("INSERT OR IGNORE INTO teams (name) VALUES (?) RETURNING id"),
        AtMostOne
    );
    assert_eq!(rows("SELECT name FROM users WHERE id = ?"), AtMostOne);
    assert_eq!(rows("SELECT name FROM users LIMIT 5, 1"), AtMostOne);
}
