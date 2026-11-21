use sqlty_sql::analyzer::*;
#[test]
fn parsed_returns_rows_as_the_free_function_did() {
    for sql in [
        "SELECT 1",
        "WITH c AS (SELECT 1) SELECT * FROM c",
        "INSERT INTO t VALUES (1)",
        "INSERT INTO t VALUES (1) RETURNING id",
        "UPDATE t SET a = 1",
        "DELETE FROM t RETURNING *",
        "CREATE TABLE t (a INT)",
        "BEGIN",
        "",
        "SELEC 1",
    ] {
        for dialect in ["postgres", "mysql", "sqlite"] {
            assert_eq!(
                parse(sql, dialect).returns_rows(),
                returns_rows(sql, dialect),
                "{sql}"
            );
        }
    }
    assert!(returns_rows("DELETE FROM t RETURNING *", "postgres"));
    assert!(returns_rows("SELEC 1", "postgres"));
    assert!(!returns_rows("INSERT INTO t VALUES (1)", "postgres"));
    assert!(!returns_rows("CREATE TABLE t (a INT)", "postgres"));
    assert!(!returns_rows("BEGIN", "postgres"));
    assert!(!returns_rows("", "postgres"));
}

#[test]
fn parsed_columns_equal_analyze_select() {
    for sql in ["SELECT 1 AS a, 'x'", "DELETE FROM t", "SELEC 1", ""] {
        assert_eq!(
            parse(sql, "postgres").columns(None),
            analyze_select(sql, "postgres", None),
            "{sql}"
        );
    }
}
