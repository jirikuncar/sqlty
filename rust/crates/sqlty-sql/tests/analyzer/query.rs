use crate::support::*;
use sqlty_sql::analyzer::{analyze_select, analyze_with_diagnostics, expand_star, Column};

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

const T: &str = "CREATE TABLE t (n INT NOT NULL, a INT, j JSONB, x INT NOT NULL, y BIGINT NOT NULL, z NUMERIC NOT NULL)";

fn columns(sql: &str, dialect: &str) -> Vec<Column> {
    let schema = load_schema(T, dialect);
    analyze_select(sql, dialect, Some(&schema)).unwrap_or_else(|| panic!("{sql} has no columns"))
}

fn types(sql: &str) -> Vec<(String, Option<String>, Option<String>)> {
    columns(sql, "postgres")
        .into_iter()
        .map(|c| (c.name.clone(), c.python_type.clone(), c.sql_type))
        .collect()
}

#[test]
fn display_type_is_optional_only_for_nullable_columns() {
    let display = |sql: &str| columns(sql, "postgres")[0].display_type();
    assert_eq!(display("SELECT n FROM t").as_deref(), Some("int"));
    assert_eq!(display("SELECT $1::int").as_deref(), Some("int"));
    assert_eq!(display("SELECT a FROM t").as_deref(), Some("int | None"));
    assert_eq!(display("SELECT nope FROM t"), None);
}

#[test]
fn display_type_does_not_double_none() {
    let display = |sql: &str| columns(sql, "postgres")[0].display_type();
    assert_eq!(
        display("SELECT j FROM t UNION SELECT NULL FROM t").as_deref(),
        Some("typing.Any")
    );
    assert_eq!(
        display("SELECT a FROM t UNION SELECT NULL FROM t").as_deref(),
        Some("int | None")
    );
}

#[test]
fn an_unreadable_wildcard_is_one_untyped_star_column() {
    let found = columns("SELECT * FROM missing", "postgres");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "*");
    assert_eq!(found[0].python_type, None);
    let found = columns("SELECT nope FROM t", "postgres");
    assert_eq!(found[0].name, "nope");
    assert_eq!(found[0].python_type, None);
    assert_eq!(
        columns("SELECT n FROM t", "postgres")[0]
            .python_type
            .as_deref(),
        Some("int")
    );
}

#[test]
fn a_column_carries_a_python_type_and_a_sql_type() {
    assert_eq!(
        types("SELECT y FROM t"),
        [(
            "y".to_string(),
            Some("int".to_string()),
            Some("BIGINT".to_string())
        )]
    );
}

#[test]
fn set_operations_take_names_from_the_leftmost_select() {
    let names = |sql: &str| -> Vec<String> {
        columns(sql, "postgres")
            .into_iter()
            .map(|c| c.name)
            .collect()
    };
    assert_eq!(names("SELECT 1 AS a"), ["a"]);
    assert_eq!(names("SELECT 1 AS a UNION SELECT 2 AS b"), ["a"]);
    assert_eq!(names("(SELECT 3 AS c) UNION SELECT 2 AS b"), ["c"]);
    assert_eq!(names("VALUES (1)"), ["column1"]);
}

#[test]
fn nested_set_operations_unify_every_branch() {
    let sql_type = |sql: &str| columns(sql, "postgres")[0].sql_type.clone();
    assert_eq!(sql_type("SELECT x FROM t").as_deref(), Some("INT"));
    assert_eq!(
        sql_type("SELECT x FROM t UNION SELECT y FROM t").as_deref(),
        Some("BIGINT")
    );
    assert_eq!(
        sql_type("SELECT x FROM t UNION SELECT y FROM t EXCEPT SELECT z FROM t").as_deref(),
        Some("DECIMAL")
    );
    assert_eq!(
        sql_type("(SELECT x FROM t UNION SELECT y FROM t) INTERSECT SELECT z FROM t").as_deref(),
        Some("DECIMAL")
    );
    assert_eq!(
        sql_type("SELECT x FROM t UNION (WITH c AS (SELECT z FROM t) SELECT * FROM c)").as_deref(),
        Some("DECIMAL")
    );
}

#[test]
fn a_table_alias_list_renames_columns_by_position() {
    let names = |dialect: &str| -> Vec<String> {
        columns("SELECT * FROM (SELECT 1, 2, 3) AS s(X, Y)", dialect)
            .into_iter()
            .map(|c| c.name)
            .collect()
    };
    assert_eq!(names("postgres"), ["x", "y", "?column?"]);
    assert_eq!(names("mysql"), ["X", "Y", "3"]);
}

#[test]
fn the_text_of_an_untyped_expression_is_cut_on_a_character_boundary() {
    let message = |expr: &str| -> String {
        let (_, diagnostics) =
            analyze_with_diagnostics(&format!("SELECT {expr}"), "postgres", None);
        diagnostics
            .iter()
            .find(|d| d.rule.name() == "unsupported-syntax")
            .map(|d| d.message.clone())
            .unwrap_or_default()
    };
    let operator = |argument: &str| format!("'{argument}' OPERATOR(pg_catalog.+) 1");
    // `'' OPERATOR(pg_catalog.+) 1` is 27 characters.
    let short = operator("a");
    assert_eq!(
        message(&short),
        format!("expression `{short}` is not typed")
    );
    let exactly = operator(&"a".repeat(33));
    assert_eq!(exactly.chars().count(), 60);
    assert_eq!(
        message(&exactly),
        format!("expression `{exactly}` is not typed")
    );
    let long = operator(&"b".repeat(34));
    let cut: String = long.chars().take(60).collect();
    assert_eq!(
        message(&long),
        format!("expression `{cut}...` is not typed")
    );
    let wide = operator(&"é".repeat(70));
    let cut: String = wide.chars().take(60).collect();
    assert_eq!(
        message(&wide),
        format!("expression `{cut}...` is not typed")
    );
}

#[test]
fn a_star_is_expanded_in_the_leftmost_select_of_a_set_operation() {
    let schema = load_schema("CREATE TABLE t (id INT NOT NULL, n TEXT)", "postgres");
    let expand = |sql: &str| expand_star(sql, "postgres", Some(&schema));
    assert_eq!(
        expand("SELECT * FROM t UNION SELECT 1, 'a'").as_deref(),
        Some("SELECT id::INT, n::TEXT FROM t UNION SELECT 1, 'a'")
    );
    assert_eq!(expand("VALUES (1)"), None);
}
