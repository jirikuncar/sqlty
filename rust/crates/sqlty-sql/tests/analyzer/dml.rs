use crate::support::*;
use sqlty_sql::analyzer::*;
#[test]
fn returning_columns_in_queries() {
    let ddl = "CREATE TABLE t (id SERIAL PRIMARY KEY, name TEXT NOT NULL, note TEXT); \
               CREATE TABLE u (id INT, extra INT)";
    let queries = [
        "INSERT INTO t (name) VALUES ('a') RETURNING id, name, note",
        "INSERT INTO t (name) VALUES ('a') RETURNING *",
        "INSERT INTO t (name) VALUES ('a') RETURNING id AS new_id, upper(name) AS shout",
        "UPDATE t SET note = 'x' RETURNING *",
        "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING *",
        "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING t.id, extra",
        "DELETE FROM t USING u WHERE t.id = u.id RETURNING t.*",
        "DELETE FROM t WHERE id = 1 RETURNING nope",
        "INSERT INTO t (name) VALUES ('a')",
        "INSERT INTO missing (a) VALUES (1) RETURNING a",
    ];
    insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
}

fn rows(sql: &str) -> bool {
    returns_rows(sql, "postgres")
}

#[test]
fn queries_return_rows() {
    assert!(rows("SELECT 1"));
    assert!(rows("VALUES (1)"));
    assert!(rows("WITH c AS (SELECT 1) SELECT * FROM c"));
}

#[test]
fn dml_returns_rows_only_with_returning() {
    assert!(!rows("INSERT INTO t (a) VALUES (1)"));
    assert!(rows("INSERT INTO t (a) VALUES (1) RETURNING a"));
    assert!(!rows("UPDATE t SET a = 1"));
    assert!(rows("UPDATE t SET a = 1 RETURNING *"));
    assert!(!rows("DELETE FROM t"));
    assert!(rows("DELETE FROM t RETURNING a"));
}

#[test]
fn a_cte_around_dml_follows_the_dml() {
    assert!(!rows(
        "WITH d AS (SELECT 1) DELETE FROM t WHERE a IN (SELECT * FROM d)"
    ));
    assert!(rows(
        "WITH d AS (SELECT 1) DELETE FROM t WHERE a IN (SELECT * FROM d) RETURNING a"
    ));
}

#[test]
fn ddl_and_transaction_control_return_no_rows() {
    assert!(!rows("CREATE TABLE t (a INT)"));
    assert!(!rows("BEGIN"));
    assert!(!rows("COMMIT"));
}

#[test]
fn merge_returns_rows_only_with_returning() {
    let merge = "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE";
    assert!(!rows(merge));
    assert!(rows(&format!("{merge} RETURNING t.id")));
}

#[test]
fn returning_lists_the_items_of_the_statement() {
    let items = |sql: &str| analyze_select(sql, "postgres", None).map(|columns| columns.len());
    assert_eq!(
        items("INSERT INTO t (a) VALUES (1) RETURNING a, b"),
        Some(2)
    );
    assert_eq!(items("UPDATE t SET a = 1 RETURNING *"), Some(1));
    assert_eq!(items("DELETE FROM t RETURNING a"), Some(1));
    assert_eq!(items("DELETE FROM t"), None);
}

#[test]
fn assigned_columns_spread_tuples() {
    let schema = load_schema("CREATE TABLE t (a INT, b TEXT, c DATE)", "postgres");
    let found = analyze_params(
        "UPDATE t SET (a, b) = ($1, $2), c = $3",
        "postgres",
        Some(&schema),
    )
    .unwrap();
    let types: Vec<_> = found.iter().map(|p| p.python_type.as_deref()).collect();
    assert_eq!(types, [Some("int"), Some("str"), Some("datetime.date")]);
}

/// The scope of a statement shows in what its `RETURNING` can name: the target, and, once
/// the statement reads other sources, names that cannot be checked.
#[test]
fn the_scope_of_dml_is_its_target_plus_extra_sources() {
    let schema = load_schema(
        "CREATE TABLE public.users (a INT); CREATE TABLE t (a INT); CREATE TABLE u (a INT, id INT)",
        "postgres",
    );
    let rule_names = |sql: &str| -> Vec<&'static str> {
        analyze_with_diagnostics(sql, "postgres", Some(&schema))
            .1
            .iter()
            .map(|d| d.rule.name())
            .collect()
    };
    let type_of = |sql: &str| infer_type(sql, "postgres", Some(&schema));
    assert_eq!(
        type_of("INSERT INTO Public.Users (a) VALUES (1) RETURNING users.a").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        rule_names("INSERT INTO Public.Users (a) VALUES (1) RETURNING nope"),
        ["unknown-column"]
    );
    assert_eq!(
        rule_names("UPDATE t SET a = 1 RETURNING nope"),
        ["unknown-column"]
    );
    assert_eq!(
        rule_names("DELETE FROM t AS x RETURNING x.a"),
        [] as [&str; 0]
    );
    assert_eq!(
        rule_names("DELETE FROM t AS x RETURNING nope"),
        ["unknown-column"]
    );
    for sql in [
        "UPDATE t SET a = u.a FROM u RETURNING u.id",
        "DELETE FROM t USING u WHERE t.a = u.a RETURNING u.id",
        "MERGE INTO t USING u ON t.a = u.a WHEN MATCHED THEN DELETE RETURNING u.id",
    ] {
        assert_eq!(rule_names(sql), [] as [&str; 0], "{sql}");
    }
}

#[test]
fn the_scope_of_an_update_adds_the_tables_of_from_and_joins() {
    let ddl = "CREATE TABLE t (a INT, id INT); CREATE TABLE u (a INT, id INT, z INT)";
    let messages = |sql: &str, dialect: &str| {
        let schema = load_schema(ddl, dialect);
        let (found, diagnostics) = analyze_with_diagnostics(sql, dialect, Some(&schema));
        let messages: Vec<_> = diagnostics.into_iter().map(|d| d.message).collect();
        (found.map(|columns| columns.len()), messages)
    };
    let said = |count: Option<usize>, said: &[&str]| {
        (
            count,
            said.iter().map(ToString::to_string).collect::<Vec<_>>(),
        )
    };
    // The target alone.
    assert_eq!(
        messages("UPDATE t SET a = 1 RETURNING nope", "postgres"),
        said(Some(1), &["column `nope` does not exist in `t`"])
    );
    // `FROM` adds its tables, which `RETURNING` reads in PostgreSQL.
    let from = "UPDATE t SET a = u.a FROM u WHERE t.id = u.id";
    assert_eq!(
        messages(&format!("{from} RETURNING u.z"), "postgres"),
        said(Some(1), &[])
    );
    assert_eq!(
        messages(&format!("{from} RETURNING nope"), "postgres"),
        said(Some(1), &["column `nope` does not exist in `t`, `u`"])
    );
    // MySQL joins them: the values may read each of them.
    assert_eq!(
        messages("UPDATE t JOIN u ON t.id = u.id SET t.a = u.nope", "mysql"),
        said(None, &["column `nope` does not exist in `u`"])
    );
}
