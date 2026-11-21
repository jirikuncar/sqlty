use crate::support::*;

const DDL: &str = "CREATE TABLE a (id INT NOT NULL, label TEXT, score NUMERIC(5, 2) NOT NULL); \
                   CREATE TABLE b (id BIGINT, label VARCHAR(20) NOT NULL, score DOUBLE PRECISION); \
                   CREATE TABLE c (id INT NOT NULL, flag BOOLEAN NOT NULL)";

#[test]
fn set_operations_unify_types() {
    let queries = [
        "SELECT id, label FROM a UNION SELECT id, label FROM b",
        "SELECT id, score FROM a UNION ALL SELECT id, score FROM b",
        "SELECT id FROM a UNION SELECT flag FROM c",
        "SELECT id FROM a UNION SELECT 1",
        "SELECT id FROM a UNION SELECT NULL",
        "SELECT NULL AS x UNION SELECT 1",
        "SELECT id FROM a UNION SELECT id, label FROM b",
        "SELECT id FROM a UNION SELECT id FROM b UNION SELECT id FROM c",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn set_operations_combine_nullability() {
    let queries = [
        "SELECT id FROM b UNION SELECT id FROM a",
        "SELECT id FROM a UNION SELECT id FROM a",
        "SELECT id FROM b INTERSECT SELECT id FROM a",
        "SELECT id FROM b INTERSECT SELECT id FROM b",
        "SELECT id FROM a EXCEPT SELECT id FROM b",
        "SELECT id FROM b EXCEPT SELECT id FROM a",
        "SELECT label FROM a UNION ALL SELECT label FROM b",
        "(SELECT id FROM a) UNION (SELECT id FROM b)",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn column_names_come_from_the_first_branch() {
    let queries = [
        "SELECT id AS first FROM a UNION SELECT id AS second FROM b",
        "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn values_lists() {
    let queries = [
        "VALUES (1, 'a'), (2, 'b')",
        "VALUES (1), (2.5)",
        "VALUES (1), (NULL)",
        "VALUES (1), (1, 2)",
        "SELECT * FROM (VALUES (1, 'a')) AS v(n, s)",
        "SELECT column1 FROM (VALUES (1)) AS v",
        "VALUES (1) UNION SELECT id FROM a",
    ];
    let postgres = reports(&queries, "postgres", Some(DDL));
    let mysql = reports(
        &[
            "VALUES ROW(1, 'a'), ROW(2, NULL)",
            "SELECT * FROM (VALUES ROW(1, 'a')) AS v",
            "SELECT column_1 FROM (VALUES ROW(1, 'a')) AS v",
            "SELECT a FROM (VALUES ROW(1, 'a')) AS v(a, b)",
        ],
        "mysql",
        None,
    );
    let sqlite = reports(
        &[
            "VALUES (1, 'a'), (2, 2.5)",
            "SELECT * FROM (VALUES (1, NULL))",
        ],
        "sqlite",
        None,
    );
    insta::assert_snapshot!(format!(
        "postgres:\n{postgres}\nmysql:\n{mysql}\nsqlite:\n{sqlite}"
    ));
}

#[test]
fn ctes() {
    let queries = [
        "WITH x AS (SELECT id FROM a) SELECT id FROM x",
        "WITH x AS (SELECT id FROM a), y AS (SELECT id FROM x WHERE id > 1) SELECT * FROM y",
        "WITH x(n) AS (SELECT id FROM a) SELECT n FROM x",
        "WITH x AS (SELECT id FROM a) SELECT id FROM x UNION SELECT id FROM b",
        "WITH x AS (SELECT 1) SELECT * FROM x",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn recursive_ctes_settle_nullability() {
    let queries = [
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5) SELECT i FROM n",
        "WITH RECURSIVE t(id, parent) AS (SELECT id, NULL::INT FROM a UNION ALL SELECT a.id, t.id FROM a JOIN t ON true) SELECT id, parent FROM t",
        "WITH RECURSIVE t(x, y) AS (SELECT 1, 2 UNION ALL SELECT y, NULL FROM t) SELECT x, y FROM t",
        "WITH RECURSIVE t AS (SELECT id FROM a UNION ALL SELECT id FROM t) SELECT * FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn projection_forms() {
    let queries = [
        "SELECT 1, 'x', true, NULL",
        "SELECT id, id AS again, label l FROM a",
        "SELECT DISTINCT label FROM a",
        "SELECT a.id, count(*) AS n FROM a GROUP BY a.id HAVING count(*) > 1 ORDER BY n DESC LIMIT 3",
        "SELECT id FROM a ORDER BY label OFFSET 1",
        "SELECT row_number() OVER (ORDER BY id) AS rn, sum(score) OVER () AS total FROM a",
        "SELECT (SELECT 1) AS one, (SELECT label FROM a LIMIT 1) AS lbl",
        "SELECT",
        "SELECT FROM a",
        "SELECT * FROM a FOR UPDATE",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn unsupported_query_forms_are_explained() {
    let queries = [
        "TABLE a",
        "SELECT id FROM a UNION TABLE b",
        "SELECT 1 UNION VALUES (2)",
    ];
    let postgres = reports(&queries, "postgres", Some(DDL));
    insta::assert_snapshot!(postgres);
}

#[test]
fn statements_that_are_not_queries_have_no_columns() {
    let queries = [
        "CREATE TABLE z (a INT)",
        "DROP TABLE a",
        "BEGIN",
        "UPDATE a SET label = 'x'",
        "SELECT FROM",
        "",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn dialects_name_and_type_the_same_query_differently() {
    let sql = ["SELECT Id, COUNT(*) AS Total, id + 1 FROM t GROUP BY Id"];
    let ddl = "CREATE TABLE t (id INT NOT NULL)";
    let rendered: Vec<String> = ["postgres", "mysql", "sqlite", "clickhouse", "bigquery"]
        .iter()
        .map(|dialect| format!("{dialect}:\n{}", reports(&sql, dialect, Some(ddl))))
        .collect();
    insta::assert_snapshot!(rendered.join("\n"));
}
