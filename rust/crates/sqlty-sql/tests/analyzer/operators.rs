use crate::support::*;
#[test]
fn binary_operators_in_queries() {
    let ddl = "CREATE TABLE t (i INT NOT NULL, s SMALLINT, b BIGINT, n NUMERIC(8, 2), \
               d DATE, ts TIMESTAMP, tz TIMESTAMPTZ, iv INTERVAL, txt TEXT, flag BOOLEAN)";
    let queries = [
        "SELECT i + s AS a, i * b AS b, s - n AS c, i / i AS d, i % s AS e FROM t",
        "SELECT i + 1.5 AS a, i + 1 AS b, n * 2 AS c FROM t",
        "SELECT d + 1 AS a, d - d AS b, d + iv AS c, ts - ts AS d, tz - iv AS e, iv * 2 AS f FROM t",
        "SELECT txt || 'x' AS a, txt || i AS b, 'a' || 'b' AS c FROM t",
        "SELECT i = 1 AS a, txt LIKE 'a%' AS b, i IS NULL AS c, NOT flag AS d, i BETWEEN 1 AND 2 AS e FROM t",
        "SELECT i IN (1, 2) AS a, i = ANY(ARRAY[1, 2]) AS b, flag AND true AS c, flag OR NULL AS d FROM t",
        "SELECT NULL + 1 AS a, 1 + NULL AS b, NULL AS c FROM t",
        "SELECT i + txt AS a FROM t",
        "SELECT 2 ^ 3 AS a, 7 & 3 AS b, 1 << 2 AS c, -i AS d, +n AS e, ~i AS f FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
}

#[test]
fn division_differs_between_postgres_and_mysql() {
    let ddl = "CREATE TABLE t (a INT NOT NULL, b INT NOT NULL)";
    let query = ["SELECT a / b AS q, a DIV b AS d, a % b AS m FROM t"];
    let postgres = reports(&query, "postgres", Some(ddl));
    let mysql = reports(&query, "mysql", Some(ddl));
    insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
}
