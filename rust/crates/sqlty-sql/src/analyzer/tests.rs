//! Analyzer unit tests that cross module boundaries.

use super::input::parser_input;
use super::preprocess::{parse_query, PARSES};
use super::*;

#[test]
fn test_parser_input() {
    assert_eq!(
        parser_input(
            "  --sql\r\nSELECT %s, %(id)s, %b, %(a-b)t, '%s', 5 %% 2",
            SqlDialect::MySql
        )
        .text(),
        "SELECT ?00, ?00, ?00, ?00, '%s', 5 % 2"
    );
    assert_eq!(
        parser_input("SELECT '100%%', 'it\\'s %s', %s", SqlDialect::MySql).text(),
        "SELECT '100%', 'it\\'s %s', ?00"
    );
    assert_eq!(
        parser_input(
            "SELECT 'a (TABLE foo', 'FOR NO KEY UPDATE', 'x MULTISET' /* (TABLE foo) */",
            SqlDialect::Postgres
        )
        .text(),
        "SELECT 'a (TABLE foo', 'FOR NO KEY UPDATE', 'x MULTISET' /* (TABLE foo) */"
    );
    assert_eq!(
        parser_input("(TABLE a) UNION ALL TABLE \"b\".c", SqlDialect::Postgres).text(),
        "(SELECT * FROM a) UNION ALL SELECT * FROM \"b\".c"
    );
    assert_eq!(parser_input("--sql SELECT 1", SqlDialect::MySql).text(), "");
    assert_eq!(
        parser_input("SELECT %d, %%(a)s", SqlDialect::MySql).text(),
        "SELECT %d, %(a)s"
    );
    assert_eq!(
        parser_input(
            "SELECT 1 FOR no key update OF a, s.b FOR KEY SHARE FOR SHARE OF \"c\" NOWAIT",
            SqlDialect::Postgres
        )
        .text(),
        "SELECT 1 FOR update OF a FOR SHARE FOR SHARE OF \"c\" NOWAIT"
    );
    assert_eq!(
        parser_input("SELECT 1 FOR NO KEY UPDATE OF a, b", SqlDialect::MySql).text(),
        "SELECT 1 FOR NO KEY UPDATE OF a, b"
    );
    assert_eq!(
        infer_type("--sql\nSELECT 1 AS x", "mysql", None).as_deref(),
        Some("tuple[int]")
    );
    // Schema DDL is not a query: a `%s` there is not a placeholder.
    assert!(load_schema("CREATE TABLE t (a INT DEFAULT %s)", "postgres").is_empty());
}

#[test]
fn test_a_query_is_parsed_once() {
    for (sql, dialect) in [
        ("SELECT 1", SqlDialect::Postgres),
        (
            "SELECT %s, 'TABLE t' FROM t FOR NO KEY UPDATE",
            SqlDialect::Postgres,
        ),
        ("SELECT 7 MOD 3", SqlDialect::MySql),
        (
            "SELECT a FROM t WHERE a BETWEEN SYMMETRIC 1 AND 2",
            SqlDialect::Postgres,
        ),
    ] {
        PARSES.with(|n| n.set(0));
        assert!(parse_query(sql, dialect).is_some(), "{sql}");
        assert_eq!(PARSES.with(std::cell::Cell::get), 1, "{sql}");
    }
}

fn without_hints(sql: &str) -> Option<String> {
    parse_function_without_hints(sql).map(|stmts| stmts[0].to_string())
}

fn without_hints_expected(plain: &str) -> Option<String> {
    let stmts = parse_ddl(plain, SqlDialect::Postgres);
    Some(stmts.expect(plain)[0].to_string())
}

#[test]
fn planner_hints_are_dropped_where_sqlparser_stops() {
    for (sql, expected) in [
        (
            "CREATE FUNCTION f(rows int, cost int) RETURNS TABLE (cost numeric) LANGUAGE sql \
             STABLE ROWS 1 COST 0.5 NOT LEAKPROOF SUPPORT pg_catalog.f_support PARALLEL SAFE \
             AS $_$ SELECT 1 FROM t ORDER BY a ROWS 2 $_$",
            "CREATE FUNCTION f(rows int, cost int) RETURNS TABLE (cost numeric) LANGUAGE sql \
             STABLE PARALLEL SAFE AS $_$ SELECT 1 FROM t ORDER BY a ROWS 2 $_$",
        ),
        (
            "CREATE FUNCTION f() RETURNS SETOF public.support LANGUAGE sql ROWS 1e+06 \
             COST /* c */ .5 SUPPORT s . \"F\" NOT -- c\n LEAKPROOF AS 'it''s'",
            "CREATE FUNCTION f() RETURNS SETOF public.support LANGUAGE sql AS 'it''s'",
        ),
        (
            "CREATE FUNCTION f(leakproof bool) RETURNS leakproof LANGUAGE sql COST +1_000 \
             SET search_path = support, leakproof SET search_path TO rows RETURN leakproof",
            "CREATE FUNCTION f(leakproof bool) RETURNS leakproof LANGUAGE sql \
             SET search_path = support, leakproof SET search_path TO rows RETURN leakproof",
        ),
    ] {
        assert_eq!(
            without_hints(sql),
            without_hints_expected(expected),
            "{sql}"
        );
    }
    assert_eq!(
        without_hints("CREATE FUNCTION f() RETURNS int LANGUAGE sql WINDOW COST 5 AS 'SELECT 1'"),
        None
    );
}

#[test]
fn planner_hints_spare_names_that_collide_with_them() {
    let pairs: Vec<&str> = include_str!("planner_hints.sql")
        .split(";\n")
        .map(str::trim)
        .filter(|sql| !sql.is_empty())
        .collect();
    assert_eq!(pairs.len(), 234);
    for pair in pairs.chunks(2) {
        assert_eq!(
            without_hints(pair[0]),
            without_hints_expected(pair[1]),
            "{}",
            pair[0]
        );
    }
}

#[test]
fn rarer_parser_inputs() {
    let text = |sql: &str| parser_input(sql, SqlDialect::Postgres).text().to_string();
    assert_eq!(text("SELECT 1 FOR"), "SELECT 1 FOR");
    assert_eq!(
        text("SELECT 1 FOR UPDATE OF (a)"),
        "SELECT 1 FOR UPDATE OF (a)"
    );
    assert_eq!(
        text("SELECT 1 FOR UPDATE OF a, (b)"),
        "SELECT 1 FOR UPDATE OF a, (b)"
    );
    assert_eq!(
        text("SELECT sum(a) OVER (ROWS BETWEEN %s PRECEDING AND CURRENT ROW EXCLUDE GROUP) FROM t"),
        "SELECT sum(a) OVER (ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING) FROM t"
    );
    let input = parser_input("--sql\nSELECT %s", SqlDialect::Postgres);
    assert_eq!(
        (input.original(8, false), input.original(9, true)),
        (13, 15)
    );
    let input = parser_input("SET CONSTRAINTS %s DEFERRED", SqlDialect::Postgres);
    assert_eq!(input.original(input.text().len(), true), 27);
}

#[test]
fn rarer_respellings() {
    let cases = [
        (
            SqlDialect::Postgres,
            "SELECT sum(a) OVER (ORDER BY a ROWS UNBOUNDED PRECEDING EXCLUDE NO OTHERS) FROM t",
            "SELECT sum(a) OVER (ORDER BY a ROWS UNBOUNDED PRECEDING ) FROM t",
        ),
        (
            SqlDialect::Postgres,
            "SELECT substring(x similar 'a' escape '#') FROM t",
            "SELECT substring(x FROM 'a' FOR '#') FROM t",
        ),
        (
            SqlDialect::Postgres,
            "SELECT XMLSERIALIZE(DOCUMENT x AS text NO INDENT) FROM t",
            "SELECT CAST( x AS text ) FROM t",
        ),
        (
            SqlDialect::Postgres,
            "SELECT XMLROOT(x, VERSION NO VALUE) FROM t",
            "SELECT XMLROOT(x,  NULL) FROM t",
        ),
        (SqlDialect::MySql, "SELECT !, 1", "SELECT !, 1"),
        (SqlDialect::MySql, "SELECT !", "SELECT !"),
        (
            SqlDialect::MySql,
            "SELECT !!a, !(b), !c.d(1)",
            "SELECT (NOT (NOT a)), (NOT (b)), (NOT c.d(1))",
        ),
        (
            SqlDialect::MySql,
            "SELECT 1, MOD(5, 2), (MOD 2)",
            "SELECT 1, MOD(5, 2), (MOD 2)",
        ),
        (
            SqlDialect::Postgres,
            "SELECT JSON_ARRAY(SELECT 1",
            "SELECT JSON_ARRAY(SELECT 1",
        ),
        (
            SqlDialect::Postgres,
            "DO LANGUAGE plpgsql 1",
            "DO LANGUAGE plpgsql 1",
        ),
        (
            SqlDialect::Postgres,
            "SELECT * FROM XMLTABLE('/a' PASSING x)",
            "SELECT * FROM XMLTABLE('/a' PASSING x)",
        ),
        (
            SqlDialect::Postgres,
            "SELECT substring(x similar 'a') FROM t",
            "SELECT substring(x FROM 'a') FROM t",
        ),
        (SqlDialect::MySql, "SELECT !'a'", "SELECT (NOT 'a')"),
        (SqlDialect::MySql, "SELECT 'a' MOD 2", "SELECT 'a' % 2"),
        (SqlDialect::MySql, "SELECT 'abc", "SELECT 'abc"),
    ];
    for (dialect, sql, read) in cases {
        assert_eq!(parser_input(sql, dialect).text(), read, "{sql}");
    }
}
