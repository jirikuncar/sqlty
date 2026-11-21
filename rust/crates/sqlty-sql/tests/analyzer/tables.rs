use sqlty_sql::analyzer::*;
fn tables(sql: &str, dialect: &str) -> Vec<String> {
    query_tables(sql, dialect)
        .into_iter()
        .map(|t| {
            let name = match t.schema {
                Some(schema) => format!("{schema}.{}", t.name),
                None => t.name,
            };
            format!("{name} {}", if t.write { "write" } else { "read" })
        })
        .collect()
}

#[test]
fn reads_and_writes() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "SELECT * FROM a JOIN s.b ON true WHERE x IN (SELECT y FROM c)",
            "postgres",
            &["a read", "s.b read", "c read"],
        ),
        (
            "WITH r AS (SELECT * FROM a) SELECT * FROM r, (SELECT 1 FROM b) d, generate_series(1, 2)",
            "postgres",
            &["a read", "b read"],
        ),
        (
            "WITH RECURSIVE t AS (SELECT 1 UNION ALL SELECT n FROM t) SELECT * FROM t",
            "postgres",
            &[],
        ),
        (
            "SELECT * FROM r WHERE EXISTS (WITH r AS (SELECT 1) SELECT * FROM r)",
            "postgres",
            &["r read"],
        ),
        (
            "INSERT INTO log (id) SELECT id FROM users",
            "postgres",
            &["log write", "users read"],
        ),
        (
            "UPDATE users u SET n = o.n FROM orders o WHERE o.id = u.id",
            "postgres",
            &["users write", "orders read"],
        ),
        (
            "DELETE FROM users USING orders WHERE orders.user_id = users.id",
            "postgres",
            &["users write", "orders read"],
        ),
        (
            "DELETE u FROM users u JOIN orders o ON o.user_id = u.id",
            "mysql",
            &["users write", "orders read"],
        ),
        (
            "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE",
            "postgres",
            &["t write", "s read"],
        ),
        (
            "UPDATE users SET n = (SELECT count(*) FROM users)",
            "postgres",
            &["users write", "users read"],
        ),
        ("SELECT * FROM", "postgres", &[]),
        (
            "WITH r AS (SELECT * FROM s.a) SELECT * FROM r JOIN b ON true, generate_series(1, 2)",
            "postgres",
            &["s.a read", "b read"],
        ),
        (
            "UPDATE users u SET n = (SELECT count(*) FROM users) FROM orders o WHERE o.id = u.id",
            "postgres",
            &["users write", "users read", "orders read"],
        ),
        (
            "DELETE u FROM users u JOIN (SELECT 1 AS id) d ON d.id = u.id",
            "mysql",
            &["users write"],
        ),
        (
            "INSERT INTO TABLE FUNCTION remote('x', t) (a) SELECT a FROM t",
            "clickhouse",
            &["t read"],
        ),
        ("SELECT * FROM IDENTIFIER('t')", "snowflake", &[]),
    ];
    for (sql, dialect, expected) in cases {
        assert_eq!(tables(sql, dialect), *expected, "{sql}");
    }
}
