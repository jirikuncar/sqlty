use crate::support::*;
use sqlty_sql::analyzer::*;
const SCHEMA: &str =
    "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b INTEGER, c TEXT, doc JSONB NOT NULL);
    CREATE TABLE u (id INTEGER PRIMARY KEY, t_id INTEGER NOT NULL, n INTEGER);";

/// One mark per result column: `N` nullable, `-` non-null, `?` unknown.
fn marks(sql: &str) -> String {
    let schema = Some(load_schema(SCHEMA, "postgres"));
    analyze_select(sql, "postgres", schema.as_deref())
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
fn test_where_narrows_columns() {
    assert_eq!(marks("SELECT a, b FROM t"), "NN");
    assert_eq!(marks("SELECT a, b FROM t WHERE a IS NOT NULL"), "-N");
    assert_eq!(marks("SELECT a, b FROM t WHERE a = 1"), "-N");
    assert_eq!(marks("SELECT a, b FROM t WHERE 1 < a"), "-N");
    assert_eq!(marks("SELECT a, b FROM t WHERE a = b"), "--");
    assert_eq!(marks("SELECT a, b FROM t WHERE a IN (1, 2)"), "-N");
    assert_eq!(marks("SELECT a, b FROM t WHERE a BETWEEN 1 AND 2"), "-N");
    assert_eq!(marks("SELECT a, c FROM t WHERE c LIKE 'x%'"), "N-");
    assert_eq!(
        marks("SELECT a, b FROM t WHERE a > 1 AND b IS NOT NULL"),
        "--"
    );
    assert_eq!(marks("SELECT a, b FROM t WHERE NOT (a IS NULL)"), "-N");
    assert_eq!(marks("SELECT a FROM t WHERE (a IS NOT NULL)"), "-");
    assert_eq!(marks("SELECT a, b FROM t WHERE a IS TRUE"), "-N");
    assert_eq!(marks("SELECT a, b FROM t WHERE a > 1 IS TRUE"), "-N");
    assert_eq!(marks("SELECT * FROM t WHERE a IS NOT NULL"), "--NN-");
}

#[test]
fn test_conditions_that_prove_nothing() {
    assert_eq!(marks("SELECT a FROM t WHERE a IS NULL"), "N");
    assert_eq!(marks("SELECT a FROM t WHERE a IS NOT DISTINCT FROM 1"), "N");
    assert_eq!(marks("SELECT a FROM t WHERE a IS NOT TRUE"), "N");
    assert_eq!(marks("SELECT a FROM t WHERE NOT (a > 1)"), "-");
    assert_eq!(marks("SELECT a FROM t WHERE a = ALL (ARRAY[1])"), "N");
    assert_eq!(marks("SELECT a FROM t WHERE a > 1 OR b > 1"), "N");
    assert_eq!(marks("SELECT a FROM t WHERE a > 1 OR a IS NOT NULL"), "-");
    assert_eq!(marks("SELECT a, b FROM t WHERE COALESCE(a, b) > 1"), "NN");
    assert_eq!(
        marks("SELECT a FROM t WHERE a IS NULL OR a > 1 AND false"),
        "N"
    );
}

#[test]
fn test_narrowing_through_expressions() {
    assert_eq!(marks("SELECT a, b FROM t WHERE a + 1 > 1"), "-N");
    assert_eq!(marks("SELECT a, c FROM t WHERE lower(c) = 'x'"), "N-");
    assert_eq!(marks("SELECT a FROM t WHERE a::BIGINT = 1"), "-");
    assert_eq!(
        marks("SELECT lower(c) FROM t WHERE lower(c) IS NOT NULL"),
        "-"
    );
    // lower(c) is only NULL for a NULL c, so upper(c) is not NULL either.
    assert_eq!(
        marks("SELECT lower(c), upper(c), c FROM t WHERE lower(c) IS NOT NULL"),
        "---"
    );
}

#[test]
fn test_json_key_test_does_not_narrow_the_accessor() {
    // `doc ? 'k'` is also true for an array holding the string 'k', where `doc -> 'k'`
    // is NULL.
    assert_eq!(
        marks("SELECT doc -> 'k', doc ->> 'k', doc -> 'j' FROM t WHERE doc ? 'k'"),
        "NNN"
    );
    // A JSON `null` passes `IS NOT NULL`, and psycopg loads it as `None`; its text does
    // not.
    assert_eq!(
        marks("SELECT doc -> 'k', doc ->> 'k' FROM t WHERE doc -> 'k' IS NOT NULL AND doc ->> 'k' IS NOT NULL"),
        "N-"
    );
    assert_eq!(marks("SELECT doc -> 'k' FROM t WHERE NOT doc ? 'k'"), "N");
}

#[test]
fn test_right_side_of_and_and_or() {
    assert_eq!(
        marks("SELECT a IS NOT NULL AND a > 1, a > 1 AND a IS NOT NULL, a IS NULL OR a > 1 FROM t"),
        "-N-"
    );
    assert_eq!(
        marks("SELECT a > 1 AND b > 1, a IS NULL AND b > 1 FROM t"),
        "NN"
    );
}

#[test]
fn test_case_branches_see_their_condition() {
    assert_eq!(
        marks("SELECT CASE WHEN a IS NOT NULL THEN a ELSE 0 END, CASE WHEN a IS NULL THEN 0 ELSE a END FROM t"),
        "--"
    );
    assert_eq!(
        marks("SELECT CASE WHEN a IS NULL THEN a ELSE 0 END, CASE WHEN a > 1 THEN a ELSE 0 END FROM t"),
        "N-"
    );
    assert_eq!(
        marks("SELECT CASE a WHEN 1 THEN a ELSE 0 END, CASE WHEN a IS NULL THEN 1 WHEN b IS NULL THEN 2 ELSE a + b END FROM t"),
        "--"
    );
    assert_eq!(marks("SELECT CASE WHEN a IS NULL THEN 1 END FROM t"), "N");
}

#[test]
fn test_inner_join_conditions_narrow() {
    // A condition on a column of an outer-joined relation drops the extended rows.
    assert_eq!(
        marks("SELECT t.id, u.id, u.n FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.id IS NOT NULL"),
        "--N"
    );
    assert_eq!(
        marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.n > 1"),
        "--"
    );
    assert_eq!(
        marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.id IS NULL"),
        "-N"
    );
    assert_eq!(
        marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE t.id = 1 OR u.id = 1"),
        "-N"
    );
    assert_eq!(
        marks("SELECT t.id, u.id FROM t RIGHT JOIN u ON u.t_id = t.id WHERE t.id = 1"),
        "--"
    );
    assert_eq!(
        marks("SELECT t.id, u.id FROM t FULL JOIN u ON u.t_id = t.id WHERE t.id = 1"),
        "-N"
    );
    // The ON of an outer join filters nothing on the preserved side.
    assert_eq!(
        marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id AND u.n IS NOT NULL"),
        "-N"
    );
    // The ON of an inner join joined later narrows an earlier outer join.
    assert_eq!(
        marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN u ON u.t_id = t.id JOIN t v ON v.id = u.n"),
        "---"
    );
    assert_eq!(
        marks("SELECT t.id, u.id FROM t LEFT JOIN u ON u.t_id = t.id WHERE u.id IS NOT NULL AND u.n IS NULL"),
        "--"
    );
    // An inner join's ON holds for the rows of its own side only: a RIGHT JOIN after
    // it null-extends them, and a LEFT JOIN before it null-extends the group.
    assert_eq!(
        marks(
            "SELECT t.id, u.id, v.id FROM t JOIN u ON u.t_id = t.id RIGHT JOIN t v ON v.id = u.n"
        ),
        "NN-"
    );
    assert_eq!(
        marks(
            "SELECT t.id, u.id, v.id FROM t LEFT JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"
        ),
        "-NN"
    );
    assert_eq!(
        marks("SELECT t.id, u.id, v.id FROM t JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"),
        "---"
    );
}

#[test]
fn test_narrowing_stays_inside_the_select() {
    // The outer WHERE does not reach into a derived table or a CTE.
    assert_eq!(
        marks("SELECT d.a FROM (SELECT a FROM t) d WHERE d.a IS NOT NULL"),
        "-"
    );
    assert_eq!(
        marks("SELECT a, (SELECT a FROM t t2 WHERE t2.id = t.id) FROM t WHERE a IS NOT NULL"),
        "-N"
    );
    assert_eq!(
        marks("SELECT a, (SELECT max(a) FROM t t2) FROM t WHERE a IS NOT NULL"),
        "-N"
    );
    // A correlated subquery reads the outer row's facts.
    assert_eq!(
        marks("SELECT (SELECT t.a) FROM t WHERE t.a IS NOT NULL"),
        "-"
    );
    assert_eq!(marks("SELECT (SELECT t.a) FROM t"), "N");
}

#[test]
fn test_having_narrows_groups() {
    assert_eq!(
        marks("SELECT a, max(b) FROM t GROUP BY a HAVING a IS NOT NULL AND max(b) IS NOT NULL"),
        "--"
    );
}
