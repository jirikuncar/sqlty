use crate::support::*;
use sqlty_sql::analyzer::analyze_select;

const DDL: &str = "CREATE TABLE t (i INT NOT NULL, s SMALLINT, b BIGINT NOT NULL, \
                   n NUMERIC(10, 2) NOT NULL, f DOUBLE PRECISION, txt TEXT, \
                   d DATE, ts TIMESTAMP NOT NULL, tz TIMESTAMPTZ, iv INTERVAL, \
                   arr INT[] NOT NULL, tags TEXT[], r INT4RANGE, j JSONB)";

#[test]
fn aggregates_in_postgres() {
    let queries = [
        "SELECT count(*) AS a, count(txt) AS b, count(DISTINCT i) AS c FROM t",
        "SELECT sum(i) AS a, sum(s) AS b, sum(b) AS c, sum(n) AS d, sum(f) AS e FROM t",
        "SELECT min(i) AS a, max(txt) AS b, min(d) AS c, max(ts) AS d FROM t",
        "SELECT avg(i) AS a, avg(f) AS b, avg(n) AS c, avg(iv) AS d FROM t",
        "SELECT sum(i) FILTER (WHERE i > 1) AS a FROM t",
        "SELECT array_agg(i) AS a, string_agg(txt, ',') AS b, bool_and(i > 1) AS c FROM t",
        "SELECT sum(nope) FROM t",
        "SELECT sum(i::BIGINT) AS a, sum(unknown_fn(i)) AS b FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn return_types_follow_the_argument() {
    let queries = [
        "SELECT abs(i) AS a, abs(n) AS b, abs(f) AS c, abs(s) AS d FROM t",
        "SELECT round(n) AS a, round(n, 1) AS b, round(f) AS c, round(i) AS d FROM t",
        "SELECT sqrt(i) AS a, sqrt(n) AS b, ceil(f) AS c, sign(n) AS d FROM t",
        "SELECT length(txt) AS a, lower(txt) AS b, concat(txt, i) AS c, substr(txt, 1) AS d FROM t",
        "SELECT coalesce(s, 1) AS a, coalesce(i, 1.5) AS b, coalesce(txt, 'x') AS c, coalesce(NULL, NULL) AS d FROM t",
        "SELECT greatest(i, b) AS a, least(i, 1) AS b, nullif(i, 0) AS c FROM t",
        "SELECT now() AS a, current_date AS b, date_trunc('day', tz) AS c, date_trunc('day', d) AS d FROM t",
        "SELECT extract(year FROM ts) AS a, age(ts) AS b, date_part('day', ts) AS c FROM t",
        "SELECT lag(i) OVER () AS a, lag(i, 1, 0) OVER () AS b, row_number() OVER () AS c FROM t",
        "SELECT lower(r) AS a, upper(r) AS b, array_length(arr, 1) AS c, unnest(arr) AS d FROM t",
        "SELECT arr[1] AS a, tags[1] AS b, array_append(arr, 1) AS c, array_agg(txt) AS d FROM t GROUP BY txt, arr, tags",
        "SELECT gen_random_uuid() AS a, md5(txt) AS b, random() AS c, pg_sleep(1) AS d FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn date_part_of_a_field_it_cannot_read_is_unknown() {
    for field in ["1", "txt"] {
        let report = report(
            &format!("SELECT date_part({field}, ts) AS c FROM t"),
            "duckdb",
            Some("CREATE TABLE t (ts TIMESTAMP NOT NULL, txt TEXT NOT NULL)"),
        );
        assert!(report.starts_with("c: ? | ? | NonNull"), "{report}");
    }
}

#[test]
fn mysql_sum_of_an_integer_is_decimal() {
    let report = report(
        "SELECT sum(i) AS total FROM t",
        "mysql",
        Some("CREATE TABLE t (i INT NOT NULL)"),
    );
    assert!(
        report.starts_with("total: decimal.Decimal | DECIMAL"),
        "{report}"
    );
}

#[test]
fn unknown_functions_are_untyped_and_explained() {
    let queries = [
        "SELECT definitely_not_a_function(i) AS a FROM t",
        "SELECT public.also_missing(i) AS a FROM t",
        "SELECT now() AS ok, mystery() AS bad FROM t",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
}

#[test]
fn function_availability_differs_by_dialect() {
    let queries = [
        "SELECT ifnull(i, 0) AS a, concat_ws(',', txt, txt) AS b, if(i > 1, 1, 0) AS c FROM t",
        "SELECT date_format(ts, '%Y') AS a, curdate() AS b, group_concat(txt) AS c FROM t",
        "SELECT avg(i) AS a, sum(i) AS b, round(n, 1) AS c, stddev(i) AS d FROM t",
    ];
    let ddl = "CREATE TABLE t (i INT NOT NULL, txt VARCHAR(10), ts DATETIME, \
               n DECIMAL(10, 2) NOT NULL)";
    let mysql = reports(&queries, "mysql", Some(ddl));
    let postgres = reports(&queries, "postgres", None);
    insta::assert_snapshot!(format!("mysql:\n{mysql}\npostgres:\n{postgres}"));
}

#[test]
fn sum_widens_in_postgres() {
    let widen = |sql: &str| {
        let declared = if sql == "DOUBLE" {
            "DOUBLE PRECISION"
        } else {
            sql
        };
        let ddl = format!("CREATE TABLE t (c {declared})");
        let schema = load_schema(&ddl, "postgres");
        analyze_select("SELECT SUM(c) AS s FROM t", "postgres", Some(&schema))
            .and_then(|columns| columns[0].sql_type.clone())
    };
    assert_eq!(widen("SMALLINT").as_deref(), Some("BIGINT"));
    assert_eq!(widen("INT").as_deref(), Some("BIGINT"));
    assert_eq!(widen("BIGINT").as_deref(), Some("DECIMAL"));
    assert_eq!(widen("DECIMAL(10, 2)").as_deref(), Some("DECIMAL"));
    assert_eq!(widen("REAL").as_deref(), Some("FLOAT"));
    assert_eq!(widen("DOUBLE").as_deref(), Some("DOUBLE"));
    assert_eq!(widen("INTERVAL").as_deref(), Some("INTERVAL"));
    assert_eq!(widen("TEXT").as_deref(), Some("TEXT"));
}

#[test]
fn extract_types_follow_the_dialect_and_field() {
    let cases = [
        ("postgres", "YEAR", Some("DECIMAL")),
        ("generic", "EPOCH", Some("DECIMAL")),
        ("mysql", "YEAR", Some("BIGINT")),
        ("duckdb", "JULIAN", Some("DOUBLE")),
        ("duckdb", "'yearweek'", Some("BIGINT")),
        ("clickhouse", "YEAR", Some("USMALLINT")),
        ("clickhouse", "NANOSECOND", Some("UINT")),
        ("clickhouse", "TIMEZONE_HOUR", Some("INT")),
        ("clickhouse", "JULIAN", None),
        ("snowflake", "YEAR", None),
    ];
    for (dialect, field, expected) in cases {
        let schema = load_schema("CREATE TABLE t (d TIMESTAMP)", dialect);
        let found = analyze_select(
            &format!("SELECT EXTRACT({field} FROM d) FROM t"),
            dialect,
            Some(&schema),
        )
        .and_then(|columns| columns[0].sql_type.clone());
        assert_eq!(found.as_deref(), expected, "{dialect} {field}");
    }
}
