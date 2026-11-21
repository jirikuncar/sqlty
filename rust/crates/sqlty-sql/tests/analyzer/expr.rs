use crate::support::*;
use sqlty_sql::analyzer::analyze_select;
const DDL: &str = "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy'); \
                   CREATE TYPE pair AS (x INT, y TEXT); \
                   CREATE DOMAIN positive AS INT CHECK (VALUE > 0); \
                   CREATE TABLE t (i INT NOT NULL, n INT, txt TEXT, m mood NOT NULL, \
                   p pair, pos positive NOT NULL, arr INT[] NOT NULL, \
                   grid INT[][], ts TIMESTAMP NOT NULL, tz TIMESTAMPTZ, j JSONB)";

#[test]
fn casts() {
    let queries = [
        "SELECT i::BIGINT AS a, i::TEXT AS b, txt::INT AS c, n::NUMERIC(10, 2) AS d",
        "SELECT CAST(i AS DOUBLE PRECISION) AS a, CAST(txt AS VARCHAR(20)) AS b FROM t",
        "SELECT '1'::INT AS a, NULL::TEXT AS b, 1::BOOLEAN AS c",
        "SELECT i::INT[] AS a, txt::TEXT[] AS b, '{}'::INT[][] AS c",
        "SELECT m::TEXT AS a, txt::mood AS b, i::positive AS c",
        "SELECT i::pair AS a",
        "SELECT i::no_such_type AS a",
        "SELECT i::JSONB AS a, i::UUID AS b, i::BYTEA AS c, i::DATE AS d",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn casts_without_a_schema_do_not_complain() {
    let queries = ["SELECT 1::no_such_type AS a", "SELECT x::INT AS a"];
    insta::assert_snapshot!(reports(&queries, "postgres", None));
}

#[test]
fn literals_and_unary_operators() {
    let queries = [
        "SELECT 1 AS a, -1 AS b, +1 AS c, 1.5 AS d, 'x' AS e, true AS f, NULL AS g",
        "SELECT -i AS a, @ i AS b, ~i AS c, NOT true AS d FROM t",
        "SELECT 1e3 AS a, 0x1F AS b, X'ff' AS c, E'a\\nb' AS d, $$dollar$$ AS e",
        "SELECT $1 AS a, $1::INT AS b",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn case_expressions() {
    let queries = [
        "SELECT CASE WHEN i > 1 THEN 1 ELSE 2 END AS a FROM t",
        "SELECT CASE WHEN i > 1 THEN 1 END AS a FROM t",
        "SELECT CASE WHEN i > 1 THEN i ELSE n END AS a FROM t",
        "SELECT CASE WHEN i > 1 THEN i ELSE 2.5 END AS a FROM t",
        "SELECT CASE WHEN i > 1 THEN i ELSE txt END AS a FROM t",
        "SELECT CASE i WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'many' END AS a FROM t",
        "SELECT CASE WHEN i > 1 THEN NULL ELSE NULL END AS a FROM t",
        "SELECT CASE WHEN i > 1 THEN m ELSE 'sad' END AS a FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn arrays_and_subscripts() {
    let queries = [
        "SELECT ARRAY[1, 2, 3] AS a, ARRAY[1, 2.5] AS b, ARRAY['a', 'b'] AS c",
        "SELECT ARRAY[i, n] AS a, ARRAY[]::INT[] AS b, ARRAY[ARRAY[1], ARRAY[2]] AS c FROM t",
        "SELECT ARRAY[1, 'a'] AS a",
        "SELECT arr[1] AS a, grid[1][2] AS b, arr[1:2] AS c, (arr)[1] AS d FROM t",
        "SELECT txt[1] AS a, i[1] AS b FROM t",
        "SELECT ARRAY(SELECT i FROM t) AS a",
        "SELECT i = ANY(arr) AS a, arr @> ARRAY[1] AS b, arr || 4 AS c FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn composite_fields_and_json_access() {
    let queries = [
        "SELECT (p).x AS a, (p).y AS b, (t.p).x AS c FROM t",
        "SELECT (p).nope AS a FROM t",
        "SELECT j -> 'a' AS a, j ->> 'a' AS b, j #> '{a,b}' AS c, j #>> '{a,b}' AS d FROM t",
        "SELECT j['a'] AS a, j @> '{}' AS b, j ? 'a' AS c FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn typed_string_literals_are_typed() {
    let report = report(
        "SELECT DATE '2020-01-01' AS d, TIMESTAMP '2020-01-01 00:00:00' AS ts",
        "postgres",
        None,
    );
    assert!(report.contains("d: datetime.date"), "{report}");
    assert!(report.contains("ts: datetime.datetime"), "{report}");
}

#[test]
fn misc_expressions() {
    let queries = [
        "SELECT EXTRACT(YEAR FROM ts) AS a, POSITION('a' IN txt) AS b, TRIM(txt) AS c FROM t",
        "SELECT SUBSTRING(txt FROM 1 FOR 2) AS a, CEIL(1.5) AS b, FLOOR(i) AS c FROM t",
        "SELECT ts AT TIME ZONE 'UTC' AS a, tz AT TIME ZONE 'UTC' AS b, i AT TIME ZONE 'UTC' AS c FROM t",
        "SELECT EXISTS (SELECT 1) AS a, i IN (SELECT i FROM t) AS b, i BETWEEN 1 AND 3 AS c FROM t",
        "SELECT (SELECT max(i) FROM t) AS a, (SELECT i, n FROM t) AS b",
        "SELECT txt ILIKE 'a%' AS a, txt SIMILAR TO 'a' AS b, txt ~ 'a' AS c FROM t",
        "SELECT i IS DISTINCT FROM n AS a, n IS NOT NULL AS b, (i > 1) IS TRUE AS c FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn mysql_and_sqlite_expressions() {
    let ddl_mysql = "CREATE TABLE t (i INT NOT NULL, txt VARCHAR(10), e ENUM('a', 'b') NOT NULL)";
    let queries = [
        "SELECT CAST(i AS CHAR) AS a, CAST(i AS SIGNED) AS b, CAST(txt AS UNSIGNED) AS c FROM t",
        "SELECT TRIM(txt) AS a, SUBSTRING(txt, 1, 2) AS b, POSITION('a' IN txt) AS c FROM t",
        "SELECT e AS a, CAST(e AS CHAR) AS b, i DIV 2 AS c FROM t",
    ];
    let mysql = reports(&queries, "mysql", Some(ddl_mysql));
    let sqlite_queries = [
        "SELECT CAST(x AS INTEGER) AS a, CAST(x AS TEXT) AS b, CAST(x AS REAL) AS c FROM t",
        "SELECT 1 AS a, 'x' AS b, x'ff' AS c FROM t",
    ];
    let sqlite = reports(&sqlite_queries, "sqlite", Some("CREATE TABLE t (x)"));
    insta::assert_snapshot!(format!("mysql:\n{mysql}\nsqlite:\n{sqlite}"));
}

/// `(python, sql)` of the only column of `sql`, against a table `t` with `ddl` columns.
fn column_of(sql: &str, ddl: &str) -> (String, String) {
    let schema = load_schema(&format!("CREATE TABLE t ({ddl})"), "postgres");
    let columns = analyze_select(sql, "postgres", Some(&schema)).expect(sql);
    (
        columns[0].python_type.clone().unwrap_or_default(),
        columns[0].sql_type.clone().unwrap_or_default(),
    )
}

#[test]
fn array_element_reads_the_inner_type() {
    let ddl = "a INT[], b INT[3], c TEXT[][], d INT";
    assert_eq!(column_of("SELECT a[1] FROM t", ddl).1, "INT");
    assert_eq!(column_of("SELECT b[1] FROM t", ddl).1, "INT");
    assert_eq!(column_of("SELECT c[1] FROM t", ddl).1, "ARRAY<TEXT>");
    assert_eq!(column_of("SELECT d[1] FROM t", ddl).1, "");
    assert_eq!(column_of("SELECT unnest(c) FROM t", ddl).1, "TEXT");
}

#[test]
fn range_element_names_the_bound_type() {
    for (range, bound) in [
        ("INT4RANGE", "INT"),
        ("INT8RANGE", "BIGINT"),
        ("NUMRANGE", "DECIMAL"),
        ("TSRANGE", "TIMESTAMP"),
        ("TSTZRANGE", "TIMESTAMPTZ"),
        ("DATERANGE", "DATE"),
        ("INT4MULTIRANGE", "INT"),
        ("DATEMULTIRANGE", "DATE"),
    ] {
        assert_eq!(
            column_of("SELECT lower(r) FROM t", &format!("r {range}")).1,
            bound,
            "{range}"
        );
    }
    // Not a range: `lower` is the string function.
    assert_eq!(column_of("SELECT lower(r) FROM t", "r INT").1, "TEXT");
}

#[test]
fn a_sql_type_gives_its_python_type_and_a_literal_has_none() {
    assert_eq!(
        column_of("SELECT 1::BIGINT FROM t", "d INT"),
        ("int".into(), "BIGINT".into())
    );
    assert_eq!(column_of("SELECT 1 FROM t", "d INT").0, "int");
    assert_eq!(column_of("SELECT 1 FROM t", "d INT").1, "");
    assert_eq!(column_of("SELECT d::NOPE FROM t", "d INT").0, "");
}
