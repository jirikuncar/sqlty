use crate::support::*;
#[test]
fn returning_columns_in_queries() {
    let ddl = "CREATE TABLE t (id SERIAL PRIMARY KEY, name TEXT NOT NULL, note TEXT); \
               CREATE TABLE u (id INT, extra INT)";
    let queries = [
        "INSERT INTO t (name) VALUES ('a') RETURNING id, name, note",
        "INSERT INTO t (name) VALUES ('a') RETURNING *",
        "INSERT INTO t (name) VALUES ('a') RETURNING id AS new_id, upper(name) AS shout",
        "UPDATE t SET note = 'x' RETURNING *",
        "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING *",
        "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING t.id, extra",
        "DELETE FROM t USING u WHERE t.id = u.id RETURNING t.*",
        "DELETE FROM t WHERE id = 1 RETURNING nope",
        "INSERT INTO t (name) VALUES ('a')",
        "INSERT INTO missing (a) VALUES (1) RETURNING a",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
}
