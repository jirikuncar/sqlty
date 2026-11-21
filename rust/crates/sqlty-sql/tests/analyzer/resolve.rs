use crate::support::*;
const DDL: &str = "CREATE TABLE users (id INT NOT NULL, name TEXT NOT NULL, note TEXT); \
                   CREATE TABLE orders (id INT NOT NULL, user_id INT NOT NULL, \
                   total NUMERIC(10, 2), note TEXT NOT NULL); \
                   CREATE VIEW user_names AS SELECT id, name FROM users; \
                   CREATE TABLE \"Case\" (\"Mixed\" INT NOT NULL, plain INT)";

#[test]
fn unqualified_names_resolve_to_the_first_source_that_has_them() {
    let queries = [
        "SELECT name FROM users",
        "SELECT name, total FROM users JOIN orders ON users.id = orders.user_id",
        "SELECT users.id, orders.id AS oid FROM users, orders",
        "SELECT u.name, o.note FROM users u JOIN orders o ON true",
        "SELECT NAME FROM users",
        "SELECT users.name FROM users AS u",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn ambiguous_names_are_reported_once_per_query() {
    let queries = [
        "SELECT id FROM users, orders",
        "SELECT note FROM users JOIN orders ON true",
        "SELECT id, note FROM users, orders",
        "SELECT id FROM users u1, users u2",
        "SELECT id FROM users JOIN orders USING (id)",
        "SELECT note FROM users NATURAL JOIN orders",
        "SELECT users.id FROM users, orders",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn unknown_names_say_what_is_missing() {
    let queries = [
        "SELECT nope FROM users",
        "SELECT users.nope FROM users",
        "SELECT x.name FROM users",
        "SELECT nope FROM missing",
        "SELECT missing.col FROM missing",
        "SELECT nope FROM users JOIN orders ON true",
        "SELECT (SELECT nope FROM users) FROM users",
        "SELECT name, nope FROM user_names",
        "SELECT nope",
        "SELECT users.* , nope.* FROM users",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn quoted_names_keep_their_case_in_postgres_only() {
    let queries = [
        "SELECT \"Mixed\" FROM \"Case\"",
        "SELECT Mixed FROM \"Case\"",
        "SELECT PLAIN FROM \"Case\"",
        "SELECT \"PLAIN\" FROM \"Case\"",
    ];
    let postgres = reports(&queries, "postgres", Some(DDL));
    let mysql_ddl = "CREATE TABLE `Case` (`Mixed` INT NOT NULL, plain INT)";
    let mysql = reports(
        &[
            "SELECT mixed FROM `Case`",
            "SELECT PLAIN FROM case_",
            "SELECT `MIXED` FROM `case`",
        ],
        "mysql",
        Some(mysql_ddl),
    );
    insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
}

#[test]
fn system_columns_exist_without_being_declared() {
    let queries = [
        "SELECT ctid, xmin, xmax, cmin, cmax, tableoid FROM users",
        "SELECT users.ctid FROM users",
        "SELECT oid FROM users",
    ];
    let postgres = reports(&queries, "postgres", Some(DDL));
    let sqlite_ddl = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)";
    let sqlite = reports(
        &[
            "SELECT rowid, _rowid_, oid FROM users",
            "SELECT ctid FROM users",
        ],
        "sqlite",
        Some(sqlite_ddl),
    );
    insta::assert_snapshot!(format!("postgres:\n{postgres}\nsqlite:\n{sqlite}"));
}

#[test]
fn nullability_comes_from_the_declaration() {
    let queries = [
        "SELECT id, note, total FROM orders",
        "SELECT o.total, u.note FROM orders o, users u",
        "SELECT name FROM user_names",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn star_over_unreadable_sources_names_the_reason() {
    let queries = [
        "SELECT * FROM missing",
        "SELECT users.* FROM users, missing",
        "SELECT * FROM users, generate_series(1, 2) AS g",
        "SELECT * FROM users, ST_Dump(NULL) AS d",
        "SELECT nope.* FROM users",
        "SELECT * FROM users JOIN orders USING (id)",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn names_are_not_judged_without_a_schema() {
    let queries = [
        "SELECT name FROM users",
        "SELECT id FROM users, orders",
        "SELECT users.* FROM users",
        "SELECT name::TEXT FROM users",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", None));
}

#[test]
fn cte_columns_are_resolved_like_tables() {
    let queries = [
        "WITH c AS (SELECT id, name FROM users) SELECT c.name, name FROM c",
        "WITH c AS (SELECT id FROM users) SELECT name FROM c",
        "WITH c AS (SELECT id FROM users), d AS (SELECT id FROM c) SELECT d.id FROM d",
        "WITH c AS (SELECT nope FROM users) SELECT * FROM c",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}
