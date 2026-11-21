//! How a schema's text is split into statements, through `analyzer::statement_ranges`.

use sqlty_sql::analyzer;

fn sqls<'a>(sql: &'a str, dialect: &str) -> Vec<&'a str> {
    analyzer::statement_ranges(sql, dialect)
        .into_iter()
        .map(|range| &sql[range])
        .collect()
}

#[test]
fn split_respects_quotes_and_bodies() {
    assert_eq!(
        sqls(
            "SELECT ';' ; -- a;b\nCREATE FUNCTION f() AS $$ x; y; $$; /* ; */ SELECT $t$;$t$",
            "postgres"
        ),
        [
            "SELECT ';'",
            "CREATE FUNCTION f() AS $$ x; y; $$",
            "SELECT $t$;$t$"
        ]
    );
    assert_eq!(
        sqls("SELECT $1; SELECT 'it''s;'", "postgres"),
        ["SELECT $1", "SELECT 'it''s;'"]
    );
    assert_eq!(
        sqls("SELECT E'a\\';b'; SELECT 1", "postgres"),
        ["SELECT E'a\\';b'", "SELECT 1"]
    );
}

#[test]
fn rarer_splits() {
    assert_eq!(sqls("SELECT 'x", "postgres"), ["SELECT 'x"]);
    assert_eq!(
        sqls("CREATE TABLE a$b (c INT); SELECT $1$", "postgres"),
        ["CREATE TABLE a$b (c INT)", "SELECT $1$"]
    );
    assert_eq!(
        sqls("/* a /* b; */ c; */ SELECT 1", "postgres"),
        ["SELECT 1"]
    );
    assert_eq!(sqls("# c;\nSELECT 1", "mysql"), ["SELECT 1"]);
}
