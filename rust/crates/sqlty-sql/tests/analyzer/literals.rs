use crate::support::*;
#[test]
fn value_sets_flow_through_expressions() {
    let ddl = "CREATE TABLE t (status TEXT NOT NULL CHECK (status IN ('new', 'done')), \
               n INT NOT NULL CHECK (n IN (1, 2)))";
    let queries = [
        "SELECT 'a' AS c, 1 AS d",
        "SELECT CASE WHEN n = 1 THEN 'x' ELSE 'y' END AS c FROM t",
        "SELECT CASE WHEN n = 1 THEN 'x' END AS c FROM t",
        "SELECT COALESCE(status, 'other') AS c FROM t",
        "SELECT 'a' AS c UNION SELECT 'b'",
        "SELECT status FROM t UNION ALL SELECT 'other'",
        "SELECT n FROM t UNION SELECT 3",
        "SELECT status || 'x' AS c, lower(status) AS d, upper('a') AS e FROM t",
        "SELECT status, n + 1 AS next FROM t",
        "SELECT * FROM (VALUES ('a'), ('b')) AS v(c)",
        "SELECT * FROM (VALUES ('a'), (1)) AS v(c)",
        "SELECT NULLIF(status, 'new') AS c FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
}
