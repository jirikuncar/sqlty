use crate::support::*;
use sqlty_sql::analyzer::*;
const SCHEMA: &str = "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b TEXT NOT NULL, c TEXT);
    CREATE TABLE u (id INTEGER PRIMARY KEY, t_id INTEGER NOT NULL, n INTEGER);";

/// One mark per result column: `N` nullable, `-` non-null, `?` unknown.
fn marks(sql: &str) -> String {
    marks_in(sql, "postgres")
}

fn marks_in(sql: &str, dialect: &str) -> String {
    marks_with(sql, dialect, SCHEMA)
}

fn marks_with(sql: &str, dialect: &str, schema: &str) -> String {
    let schema = Some(load_schema(schema, dialect));
    analyze_select(sql, dialect, schema.as_deref())
        .unwrap()
        .iter()
        .map(|col| match col.nullability {
            Nullability::Nullable => 'N',
            Nullability::NonNull => '-',
            Nullability::Unknown => '?',
        })
        .collect()
}

#[test]
fn test_columns_follow_the_schema() {
    assert_eq!(marks("SELECT id, a, b, c FROM t"), "-N-N");
    assert_eq!(marks("SELECT * FROM t"), "-N-N");
    assert_eq!(marks("SELECT t.id, x.c FROM t, t AS x"), "-N");
    assert_eq!(marks("SELECT nope FROM t"), "?");
    assert_eq!(marks("SELECT id FROM missing"), "?");
}

#[test]
fn test_outer_joins_extend_relations() {
    let join = |kind: &str| {
        marks(&format!(
            "SELECT t.id, u.id FROM t {kind} JOIN u ON u.t_id = t.id"
        ))
    };
    assert_eq!(join("INNER"), "--");
    assert_eq!(join("LEFT"), "-N");
    assert_eq!(join("RIGHT"), "N-");
    assert_eq!(join("FULL"), "NN");
    assert_eq!(
        marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN u ON u.t_id = t.id LEFT JOIN t v ON v.id = u.n"),
        "-NN"
    );
    assert_eq!(
        marks(
            "SELECT t.id, u.id, v.id FROM t LEFT JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"
        ),
        "-NN"
    );
    assert_eq!(
        marks("SELECT * FROM t LEFT JOIN u ON u.t_id = t.id"),
        "-N-NNNN"
    );
}

#[test]
fn test_literals_casts_and_operators() {
    assert_eq!(marks("SELECT 1, 'x', true, NULL"), "---N");
    assert_eq!(
        marks("SELECT a::BIGINT, CAST(b AS TEXT), (a) FROM t"),
        "N-N"
    );
    assert_eq!(
        marks("SELECT a + 1, b || 'x', -a, NOT (a > 1) FROM t"),
        "N-NN"
    );
    assert_eq!(
        marks("SELECT a IS NULL, a IS NOT NULL, a IS DISTINCT FROM 1 FROM t"),
        "---"
    );
    assert_eq!(
        marks("SELECT a IN (1, 2), id IN (1, NULL), b LIKE 'x%' FROM t"),
        "NN-"
    );
    assert_eq!(marks("SELECT $1, nosuch(1) FROM t"), "??");
    assert_eq!(marks("SELECT EXISTS (SELECT 1)"), "-");
}

#[test]
fn test_functions() {
    assert_eq!(
        marks("SELECT coalesce(a, 0), coalesce(a, c::INT), coalesce(a, id) FROM t"),
        "-N-"
    );
    assert_eq!(
        marks("SELECT nullif(id, 0), lower(c), lower(b), length(b) FROM t"),
        "NN--"
    );
    assert_eq!(marks("SELECT now(), random(), lower('x') FROM t"), "---");
    assert_eq!(marks("SELECT concat(c, b), concat_ws(',', c) FROM t"), "--");
    assert_eq!(
        marks_in("SELECT concat(c, b), concat_ws(',', c) FROM t", "mysql"),
        "N-"
    );
    assert_eq!(
        marks("SELECT greatest(a, 1), least(a, c::INT) FROM t"),
        "-N"
    );
    assert_eq!(
        marks_in("SELECT greatest(a, 1), ifnull(a, 1) FROM t", "mysql"),
        "N-"
    );
    assert_eq!(
        marks_in("SELECT if(a > 1, b, c), ifnull(c, 'x') FROM t", "mysql"),
        "N-"
    );
    assert_eq!(
        marks_in(
            "SELECT iif(a > 1, b, 'x'), coalesce(c, 'x') FROM t",
            "sqlite"
        ),
        "--"
    );
    assert_eq!(marks("SELECT user_defined(1) FROM t"), "?");
}

#[test]
fn test_division_by_zero_is_null_outside_postgres() {
    let sql = "SELECT b / 2, b / b, id % id, id % 2, mod(id, id), mod(id, 2), id DIV id, id DIV 2, id <=> a FROM u2";
    let schema = "CREATE TABLE u2 (id INT NOT NULL, b INT NOT NULL, a INT);";
    assert_eq!(marks_with(sql, "mysql", schema), "-NN-N-N--");
    assert_eq!(
        marks_with(
            "SELECT b / 2, b / b, id % id, id % 2 FROM u2",
            "sqlite",
            schema
        ),
        "-NN-"
    );
    assert_eq!(
        marks_with(
            "SELECT b / b, id % id, id <=> a FROM u2",
            "postgres",
            schema
        ),
        "--N"
    );
}

#[test]
fn test_functions_that_can_miss() {
    assert_eq!(
        marks("SELECT regexp_match(b, 'a'), regexp_substr(b, 'a'), regexp_replace(b, 'a', 'b'), substring(b FROM 'a'), substring(b FROM 2), substring(b FROM $1 FOR 2), current_setting('x', true), current_setting('x') FROM t"),
        "NN-N-NN-"
    );
}

#[test]
fn test_array_elements_can_be_null() {
    let schema = "CREATE TABLE d (id INT NOT NULL, nums INT[] NOT NULL);";
    let run = |sql: &str| marks_with(sql, "postgres", schema);
    assert_eq!(
        run("SELECT id = ANY (nums), id = ALL (nums), id IN (SELECT unnest(nums)), unnest(nums) FROM d"),
        "NNNN"
    );
    assert_eq!(
        run(
            "SELECT id = ANY (ARRAY[1, 2]), id = ANY (ARRAY[1, NULL]), id < ALL (ARRAY[id]) FROM d"
        ),
        "-N-"
    );
}

#[test]
fn test_json_and_array_accessors_can_be_null() {
    let schema = "CREATE TABLE d (doc JSONB NOT NULL, tags TEXT[] NOT NULL);";
    let run = |sql: &str| marks_with(sql, "postgres", schema);
    assert_eq!(
        run("SELECT doc -> 'k', doc ->> 'k', doc #>> '{a,b}' FROM d"),
        "NNN"
    );
    assert_eq!(
        run("SELECT tags[1], array_length(tags, 1), regexp_match('a', 'b') FROM d"),
        "NNN"
    );
    assert_eq!(run("SELECT doc, tags FROM d"), "--");
}

#[test]
fn test_aggregates() {
    assert_eq!(
        marks("SELECT count(*), count(a), max(id), sum(b::INT), min(a) FROM t"),
        "--NNN"
    );
    assert_eq!(
        marks("SELECT count(*), max(id), avg(id), array_agg(a) FROM t"),
        "-NNN"
    );
    // Every group has a row, so only a nullable argument yields NULL.
    assert_eq!(
        marks("SELECT b, count(*), max(id), max(a), array_agg(a) FROM t GROUP BY b"),
        "---N-"
    );
    // A FILTER can leave a group empty.
    assert_eq!(
        marks("SELECT b, max(id) FILTER (WHERE a > 1) FROM t GROUP BY b"),
        "-N"
    );
    // Percentiles, correlations and sample statistics can be NULL for any group.
    assert_eq!(
        marks("SELECT b, percentile_cont(0.5) WITHIN GROUP (ORDER BY id), percentile_disc(0.5) WITHIN GROUP (ORDER BY id), corr(id, id), covar_samp(id, id), covar_pop(id, id), regr_slope(id, id), stddev(id), var_samp(id), regr_count(id, id), mode() WITHIN GROUP (ORDER BY id) FROM t GROUP BY b"),
        "-NNNNNNNN-N"
    );
    // GROUP BY () is a single group over possibly no rows.
    assert_eq!(marks("SELECT max(id) FROM t GROUP BY ()"), "N");
    assert_eq!(marks("SELECT max(id) FROM t GROUP BY ROLLUP (b)"), "N");
}

#[test]
fn test_window_functions() {
    assert_eq!(
        marks("SELECT row_number() OVER (ORDER BY id), rank() OVER (ORDER BY id), count(a) OVER () FROM t"),
        "---"
    );
    assert_eq!(
        marks("SELECT sum(id) OVER (ORDER BY id), sum(a) OVER (), max(id) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND 1 PRECEDING) FROM t"),
        "-NN"
    );
    assert_eq!(
        marks("SELECT last_value(id) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING), last_value(id) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 2 FOLLOWING), first_value(id) OVER (ROWS BETWEEN 2 PRECEDING AND CURRENT ROW), first_value(id) OVER (ROWS BETWEEN 2 PRECEDING AND 1 PRECEDING), first_value(id) OVER (ROWS 2 PRECEDING), last_value(id) OVER w FROM t WINDOW w AS (ORDER BY id)"),
        "-N-N-N"
    );
    assert_eq!(
        marks("SELECT lag(id) OVER (ORDER BY id), lag(id, 1, 0) OVER (ORDER BY id), lead(a, 1, 0) OVER (ORDER BY id), first_value(id) OVER (ORDER BY id) FROM t"),
        "N-N-"
    );
}

#[test]
fn test_case_without_else_can_be_null() {
    assert_eq!(
        marks("SELECT CASE WHEN a > 1 THEN b END, CASE WHEN a > 1 THEN b ELSE 'x' END, CASE WHEN a > 1 THEN c ELSE 'x' END FROM t"),
        "N-N"
    );
    assert_eq!(
        marks("SELECT CASE a WHEN 1 THEN 'x' ELSE NULL END FROM t"),
        "N"
    );
}

#[test]
fn test_scalar_subqueries() {
    // Zero rows read as NULL.
    assert_eq!(
        marks("SELECT (SELECT b FROM u WHERE u.id = t.id) FROM t"),
        "N"
    );
    assert_eq!(marks("SELECT (SELECT id FROM t LIMIT 1) FROM t"), "N");
    // One row, whose column decides.
    assert_eq!(
        marks("SELECT (SELECT 1), (SELECT NULL), (SELECT count(*) FROM u) FROM t"),
        "-N-"
    );
    assert_eq!(
        marks("SELECT (SELECT max(id) FROM u), (SELECT count(*) + 1 FROM u) FROM t"),
        "N-"
    );
    assert_eq!(
        marks("SELECT (SELECT sum(id) FROM u WHERE u.t_id = t.id) FROM t"),
        "N"
    );
    assert_eq!(
        marks("SELECT id IN (SELECT t_id FROM u), id IN (SELECT n FROM u) FROM t"),
        "-N"
    );
}
