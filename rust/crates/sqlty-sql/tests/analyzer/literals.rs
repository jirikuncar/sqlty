use crate::support::*;
use sqlty_sql::analyzer::analyze_select;
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

/// `python | sql` of the first column of `sql`.
fn first_column(sql: &str, dialect: &str, ddl: &str) -> String {
    let schema = load_schema(ddl, dialect);
    let columns = analyze_select(sql, dialect, Some(&schema)).expect(sql);
    format!(
        "{} | {}",
        columns[0].python_type.as_deref().unwrap_or("?"),
        columns[0].sql_type.as_deref().unwrap_or("?")
    )
}

fn pg(sql: &str) -> String {
    first_column(sql, "postgres", "CREATE TABLE t (a TEXT, n INT)")
}

#[test]
fn a_constant_is_a_literal_without_a_sql_type() {
    assert_eq!(pg("SELECT 'a'"), "typing.Literal['a'] | ?");
    let negative = first_column(
        "SELECT n FROM t",
        "postgres",
        "CREATE TABLE t (n INT NOT NULL CHECK (n IN (-3, 1)))",
    );
    assert_eq!(negative, "typing.Literal[-3, 1] | INT");
}

#[test]
fn a_literal_collapses_to_its_base_in_a_function_or_operator() {
    assert_eq!(pg("SELECT upper('a')"), "str | TEXT");
    assert_eq!(pg("SELECT 'a' || 'b'"), "str | TEXT");
    assert_eq!(pg("SELECT 1 + 1"), "int | INT");
}

#[test]
fn a_column_without_a_value_set_has_no_literals() {
    assert_eq!(pg("SELECT a FROM t"), "str | TEXT");
    assert_eq!(
        pg("SELECT a, 'x' FROM t UNION SELECT a, 'y' FROM t"),
        "str | TEXT"
    );
}

#[test]
fn a_union_unites_two_sets() {
    assert_eq!(
        pg("SELECT CASE WHEN n = 1 THEN 'a' ELSE 'b' END FROM t UNION \
            SELECT CASE WHEN n = 1 THEN 'b' ELSE 'c' END FROM t"),
        "typing.Literal['a', 'b', 'c'] | ?"
    );
}

#[test]
fn a_union_of_a_set_and_a_plain_column_gives_the_plain_type() {
    assert_eq!(pg("SELECT 'a' UNION SELECT a FROM t"), "str | TEXT");
}

#[test]
fn a_union_of_strings_and_integers_follows_the_dialect() {
    let ddl = "CREATE TABLE t (a TEXT)";
    let union = "SELECT 'a' UNION SELECT 1";
    assert_eq!(
        first_column(union, "postgres", ddl),
        "int | ?",
        "PostgreSQL reads a string constant as `unknown`, which takes the integer type"
    );
    assert_eq!(
        first_column(union, "mysql", ddl),
        "str | TEXT",
        "MySQL converts the integer to a string"
    );
    assert_eq!(
        first_column(union, "sqlite", ddl),
        "str | int | ?",
        "SQLite returns either value"
    );
}

#[test]
fn a_set_past_the_cap_widens_to_its_base() {
    // 32 constants are a set; one more is not.
    let numbers = |from: usize, to: usize| {
        (from..to)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let sql = format!(
        "SELECT x FROM (VALUES ({0})) v(x) UNION SELECT y FROM (VALUES ({1})) w(y)",
        numbers(0, 1),
        numbers(1, 2)
    );
    assert!(pg(&sql).starts_with("typing.Literal[0, 1]"), "{}", pg(&sql));
    let many = |from: usize| {
        (from..from + 32)
            .map(|n| format!("SELECT {n}"))
            .collect::<Vec<_>>()
            .join(" UNION ")
    };
    assert_eq!(pg(&format!("{} UNION {}", many(0), many(1))), "int | ?");
}

#[test]
fn a_union_keeps_every_label_of_an_enum() {
    let labels: Vec<String> = (0..33).map(|n| format!("'v{n}'")).collect();
    let ddl = format!(
        "CREATE TYPE big_enum AS ENUM ({}); CREATE TABLE t (e big_enum NOT NULL)",
        labels.join(", ")
    );
    let found = first_column("SELECT e FROM t UNION SELECT e FROM t", "postgres", &ddl);
    let (python, sql) = found.split_once(" | ").unwrap();
    assert_eq!(sql, "BIG_ENUM");
    assert_eq!(python.matches("'v").count(), 33, "{python}");
}
