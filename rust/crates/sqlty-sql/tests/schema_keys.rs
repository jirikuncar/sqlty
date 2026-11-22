//! The keys a schema derives, read through the lints that depend on them: `unproven-join`
//! for the unique keys of a table and `entity-mismatch` for the table a foreign key leads to.

use sqlty_sql::analyzer::{analyze_with_diagnostics, load_schema_with_skipped};
use sqlty_sql::diagnostics::Rule;

/// The rules `sql` reports against `ddl`.
fn rules(ddl: &str, dialect: &str, sql: &str) -> Vec<Rule> {
    let (schema, _) = load_schema_with_skipped(ddl, dialect);
    let (_, diagnostics) = analyze_with_diagnostics(sql, dialect, Some(&schema));
    diagnostics.into_iter().map(|d| d.rule).collect()
}

/// Whether joining `table` with itself on `columns` is reported as matching no unique key.
fn unproven(ddl: &str, dialect: &str, table: &str, columns: &[&str]) -> bool {
    let on = |left: &str, right: &str| {
        columns
            .iter()
            .map(|c| format!("{left}.{c} = {right}.{c}"))
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    // The aggregate makes the lint look at how often each join repeats rows.
    let sql = format!(
        "SELECT SUM(LENGTH(z.{first})) FROM {table} p JOIN {table} q ON {} JOIN {table} z ON {}",
        on("q", "p"),
        on("z", "p"),
        first = columns[0],
    );
    rules(ddl, dialect, &sql).contains(&Rule::UnprovenJoin)
}

#[test]
fn unique_keys_follow_constraints_indexes_and_drops() {
    let ddl = "CREATE TABLE t (a int PRIMARY KEY, b int UNIQUE, c int, d int, e int, \
         CONSTRAINT t_cd UNIQUE (c, d)); \
         CREATE UNIQUE INDEX t_e ON t (e); CREATE UNIQUE INDEX t_lower ON t (lower(e::text)); \
         CREATE UNIQUE INDEX t_partial ON t (c) WHERE d > 0; \
         ALTER TABLE t DROP CONSTRAINT t_cd; ALTER TABLE t ADD UNIQUE (d); \
         ALTER TABLE t RENAME COLUMN e TO f;";
    // The keys are `a`, `b` and `d`: not `c` (a partial index, a dropped constraint), not the
    // expression index, and not the renamed `f`.
    for (column, unique) in [
        ("a", true),
        ("b", true),
        ("c", false),
        ("d", true),
        ("f", false),
    ] {
        assert_eq!(
            !unproven(ddl, "postgres", "t", &[column]),
            unique,
            "{column}"
        );
    }
    let dropped = "CREATE TABLE t (a int); CREATE UNIQUE INDEX t_a ON t (a); DROP INDEX t_a;";
    assert!(unproven(dropped, "postgres", "t", &["a"]));
    let sqlite = "CREATE TABLE t (a TEXT, b TEXT, PRIMARY KEY (a, b));";
    assert!(!unproven(sqlite, "sqlite", "t", &["a", "b"]));
    assert!(unproven(sqlite, "sqlite", "t", &["a"]));
    let mysql = "CREATE TABLE t (a INT, UNIQUE KEY t_a (a));";
    assert!(!unproven(mysql, "mysql", "t", &["a"]));
}

#[test]
fn entities_follow_foreign_keys_to_a_key() {
    let ddl = "CREATE TABLE a (id int PRIMARY KEY, note int); \
         CREATE TABLE b (id int PRIMARY KEY REFERENCES a, a_id int REFERENCES a (id)); \
         CREATE TABLE c (b_id int REFERENCES b); \
         CREATE TABLE x (id int PRIMARY KEY, y_id int UNIQUE); \
         CREATE TABLE y (id int PRIMARY KEY, x_id int UNIQUE REFERENCES x (y_id)); \
         ALTER TABLE x ADD FOREIGN KEY (y_id) REFERENCES y (x_id);";
    let mismatch = |sql: &str| rules(ddl, "postgres", sql).contains(&Rule::EntityMismatch);
    // `c.b_id` and `b.a_id` lead through foreign keys to `a.id`: they hold `a` keys, as `a.id`.
    assert!(!mismatch("SELECT 1 FROM c JOIN a ON c.b_id = a.id"));
    assert!(!mismatch("SELECT 1 FROM b JOIN a ON b.a_id = a.id"));
    assert!(mismatch("SELECT 1 FROM c JOIN x ON c.b_id = x.id"));
    assert!(mismatch("SELECT 1 FROM b JOIN x ON b.a_id = x.id"));
    // A column that is no key's reference has no entity, and neither has a cycle's.
    assert!(!mismatch("SELECT 1 FROM a a1 JOIN x ON a1.note = x.id"));
    assert!(!mismatch("SELECT 1 FROM x JOIN a ON x.y_id = a.id"));
}

#[test]
fn lookups_of_a_view_or_an_unknown_name_find_nothing() {
    let ddl = "CREATE TABLE t (a int PRIMARY KEY); CREATE TABLE w (id int PRIMARY KEY);
        CREATE VIEW v AS SELECT a FROM t;
        CREATE TABLE u (b int REFERENCES v (a));
        CREATE FUNCTION f(x int) RETURNS int LANGUAGE sql AS 'SELECT 1';
        CREATE FUNCTION f(x text) RETURNS SETOF int LANGUAGE sql AS 'SELECT 1';";
    // A key to a view roots in no table.
    assert!(
        !rules(ddl, "postgres", "SELECT 1 FROM u JOIN w ON u.b = w.id")
            .contains(&Rule::EntityMismatch)
    );
    // Two functions of one name that differ in kind answer no call.
    let (schema, _) = load_schema_with_skipped(ddl, "postgres");
    let (found, _) = analyze_with_diagnostics("SELECT f(1)", "postgres", Some(&schema));
    assert_eq!(found.unwrap_or_default()[0].python_type, None);
}
