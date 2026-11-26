//! Analyzer unit tests that cross module boundaries.

use super::input::parser_input;
use super::preprocess::{
    numbered_placeholders, parse_first, parse_query, sqlite_type_names, PARSES,
};
use super::query::Analyzer;
use super::*;

fn infer(sql: &str) -> Option<String> {
    infer_type(sql, "postgres", None)
}

#[test]
fn sum_of_a_column_missing_from_the_schema_is_untyped() {
    let (schema, _) = load_schema_with_skipped("CREATE TABLE users (id INT)", "postgres");
    let (columns, diagnostics) =
        analyze_with_diagnostics("SELECT SUM(nope) FROM users", "postgres", Some(&schema));
    assert_eq!(columns.unwrap()[0].python_type, None);
    assert_eq!(diagnostics[0].rule.name(), "unknown-column");
}

#[test]
fn unparsed_statements_that_set_keys_or_types_are_skipped() {
    let long = format!(
        "ALTER TABLE t ADD CONSTRAINT {} UNIQUE (id) FOO",
        "c".repeat(80)
    );
    let schema = format!(
        "CREATE TABLE t (id INT, meta TEXT);
         ALTER TABLE t ALTER COLUMN meta SET NOT NULL FOO;
         ALTER TABLE t ALTER COLUMN meta SET STATISTICS 200;
         CREATE TYPE mood AS ENUM ('a') FOO;
         GRANT SELECT ON t TO someone FOO;
         {long};"
    );
    let (_, skipped) = load_schema_with_skipped(&schema, "postgres");
    assert_eq!(
        skipped,
        [
            (
                "statement",
                "ALTER TABLE t ALTER COLUMN meta SET NOT NULL FOO".to_string()
            ),
            (
                "statement",
                "CREATE TYPE mood AS ENUM ('a') FOO".to_string()
            ),
            ("statement", format!("{}...", &long[..80])),
        ]
    );
}

#[test]
fn test_dml_returning() {
    assert_eq!(
        infer("DELETE FROM t RETURNING id::INTEGER").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("WITH x AS (SELECT 1) UPDATE t SET a = 1 RETURNING id::INTEGER").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer("INSERT INTO t SELECT a::INTEGER FROM u"), None);
    assert_eq!(
        infer(
            "INSERT INTO t (a) VALUES (1) ON CONFLICT (a) WHERE b = 'x' DO NOTHING RETURNING a::INTEGER"
        )
        .as_deref(),
        Some("tuple[int]")
    );
}

#[test]
fn test_empty_target_list() {
    assert_eq!(
        infer("SELECT FROM t FOR UPDATE").as_deref(),
        Some("tuple[()]")
    );
    assert_eq!(infer("SELECT FROM t").as_deref(), Some("tuple[()]"));
}

#[test]
fn test_schema_order_and_canonical_types() {
    let schema = Some(load_schema(
        "CREATE TABLE users (id INTEGER NOT NULL, score NUMERIC(10,2) NOT NULL, tags TEXT[] NOT NULL)",
        "postgres",
    ));
    assert_eq!(
        expand_star("SELECT * FROM users", "postgres", schema.as_deref()).as_deref(),
        Some("SELECT id::INT, score::DECIMAL(10, 2), tags::ARRAY<TEXT> FROM users")
    );
    assert_eq!(
        infer_type("SELECT * FROM users", "postgres", schema.as_deref()).as_deref(),
        Some("tuple[int, decimal.Decimal, list[str]]")
    );
}

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

fn names(sql: &str) -> Vec<String> {
    names_in(sql, "postgres")
}

#[test]
fn test_unaliased_names() {
    assert_eq!(
        names("SELECT NOT active, -n, -1, n::INT, COUNT(*), lower(x), f(x) FROM t"),
        ["?column?", "?column?", "?column?", "n", "count", "lower", "f"]
    );
}

#[test]
fn test_mysql_column_types() {
    let schema = Some(load_schema(
        "CREATE TABLE t (a INT3 ZEROFILL, b INT ZEROFILL UNSIGNED, \
         c INT SIGNED ZEROFILL, d MIDDLEINT(8) UNSIGNED)",
        "mysql",
    ));
    let types: Vec<_> = analyze_select("SELECT * FROM t", "mysql", schema.as_deref())
        .unwrap()
        .into_iter()
        .map(|col| col.sql_type.unwrap())
        .collect();
    assert_eq!(types, ["UMEDIUMINT", "UINT", "UINT", "UMEDIUMINT(8)"]);
    assert_eq!(
        expand_star("SELECT * FROM t", "mysql", schema.as_deref()).as_deref(),
        Some("SELECT a, b, c, d FROM t"),
        "MySQL has no `::`"
    );
    assert_eq!(
        names_in("SELECT 1 zerofill", "mysql"),
        ["zerofill"],
        "queries are not respelled"
    );
}

fn names_in(sql: &str, dialect: &str) -> Vec<String> {
    analyze_select(sql, dialect, None)
        .unwrap()
        .into_iter()
        .map(|col| col.name)
        .collect()
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

#[test]
fn test_numbered_placeholders() {
    assert_eq!(
        numbered_placeholders(
            "SELECT '?', \"a?\", b ?| c, d ?& e FROM t WHERE x = ? -- ?\nAND y = ?"
        ),
        "SELECT '?', \"a?\", b ?| c, d ?& e FROM t WHERE x = $1 -- ?\nAND y = $2"
    );
}

#[test]
fn test_question_mark_placeholders() {
    assert_eq!(
        infer("SELECT a::INT FROM t WHERE id = ? AND b ?| array['x']").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT a::INT FROM t WHERE b ? 'x'").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer("SELECT a::INT FROM t WHERE b ? 'x' AND id = ?"), None);
}

#[test]
fn test_dialect_type_aliases() {
    let types = |columns: &str, dialect: &str| -> Vec<String> {
        let schema = Some(load_schema(&format!("CREATE TABLE t ({columns})"), dialect));
        analyze_select("SELECT * FROM t", dialect, schema.as_deref())
            .unwrap()
            .into_iter()
            .map(|col| col.sql_type.unwrap())
            .collect()
    };
    assert_eq!(
        types("a INT64, b FLOAT64", "bigquery"),
        ["BIGINT", "DOUBLE"]
    );
    assert_eq!(
        types("a Int8, b INT8", "clickhouse"),
        ["TINYINT", "TINYINT"]
    );
    assert_eq!(types("a INT8", "postgres"), ["BIGINT"]);
    assert_eq!(types("a UNIQUEIDENTIFIER", "tsql"), ["UUID"]);
}

#[test]
fn star_expansion_casts_only_where_the_dialect_has_a_cast_operator() {
    let expand = |dialect: &str| {
        let schema = Some(load_schema("CREATE TABLE t (a INTEGER, b TEXT)", dialect));
        expand_star("SELECT * FROM t", dialect, schema.as_deref())
    };
    for dialect in ["postgres", "duckdb", "snowflake"] {
        assert_eq!(
            expand(dialect).as_deref(),
            Some("SELECT a::INT, b::TEXT FROM t"),
            "{dialect}"
        );
    }
    for dialect in ["mysql", "sqlite", "bigquery", "tsql"] {
        assert_eq!(
            expand(dialect).as_deref(),
            Some("SELECT a, b FROM t"),
            "{dialect}"
        );
    }
}

#[test]
fn test_sum_of_unknown_schema_column_is_untyped() {
    let schema = Some(load_schema("CREATE TABLE users (id INT)", "postgres"));
    assert_eq!(
        infer_type("SELECT SUM(nope) FROM users", "postgres", schema.as_deref()),
        None
    );
}

#[test]
fn test_ctes_and_derived_tables() {
    let schema = Some(load_schema(
        "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, born DATE NOT NULL)",
        "postgres",
    ));
    let infer = |sql: &str| infer_type(sql, "postgres", schema.as_deref());
    assert_eq!(
        infer("WITH r AS (SELECT id, name FROM users) SELECT id, name FROM r").as_deref(),
        Some("tuple[int, str]")
    );
    assert_eq!(
        infer("WITH r(a, b) AS (SELECT id, name FROM users) SELECT b, a FROM r").as_deref(),
        Some("tuple[str, int]")
    );
    assert_eq!(
        infer("WITH a AS (SELECT id FROM users), b AS (SELECT id FROM a) SELECT * FROM b")
            .as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT t.* FROM (SELECT name, born FROM users) t").as_deref(),
        Some("tuple[str, datetime.date]")
    );
    assert_eq!(
        infer("SELECT x FROM (SELECT id FROM users) t(x)").as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("SELECT u.id, o.name FROM users u JOIN LATERAL (SELECT name FROM users WHERE id = u.id) o ON true")
            .as_deref(),
        Some("tuple[int, str]")
    );
    assert_eq!(
        infer("WITH n AS (INSERT INTO users (name) VALUES ('a') RETURNING id, name) SELECT id, name FROM n")
            .as_deref(),
        Some("tuple[int, str]")
    );
    assert_eq!(
        infer("WITH RECURSIVE c AS (SELECT id FROM users UNION ALL SELECT id FROM c) SELECT id FROM c")
            .as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(
        infer("WITH users AS (SELECT 1 AS other) SELECT id FROM users"),
        None
    );
}

#[test]
fn test_set_operation_types() {
    let schema = Some(load_schema(
        "CREATE TABLE a (n INTEGER, t TEXT, d DATE);\
         CREATE TABLE b (n BIGINT, t VARCHAR(5), d TIMESTAMP);\
         CREATE TABLE c (g GEOMETRY);",
        "postgres",
    ));
    let sql_types = |sql: &str| -> Vec<Option<String>> {
        Analyzer::new(schema.as_deref().unwrap(), SqlDialect::Postgres)
            .statement_columns(&parse_first(sql, SqlDialect::Postgres).unwrap())
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|col| col.sql_type)
            .collect()
    };
    let types = |names: &[&str]| -> Vec<Option<String>> {
        names
            .iter()
            .map(|name| (!name.is_empty()).then(|| name.to_string()))
            .collect()
    };
    assert_eq!(
        sql_types("SELECT n, t, d FROM a UNION SELECT n, t, d FROM b"),
        types(&["BIGINT", "TEXT", "TIMESTAMP"])
    );
    assert_eq!(
        sql_types("SELECT NULL, $1, n FROM a INTERSECT SELECT n, t, n::REAL FROM b"),
        types(&["BIGINT", "VARCHAR(5)", "FLOAT"])
    );
    assert_eq!(
        sql_types("SELECT n, t FROM a EXCEPT (SELECT t, n FROM b UNION SELECT n, t FROM b)"),
        types(&["", ""])
    );
    assert_eq!(
        sql_types("SELECT (SELECT n FROM a UNION ALL SELECT n FROM b)"),
        types(&["BIGINT"])
    );
    assert_eq!(
        sql_types(
            "WITH RECURSIVE r AS (SELECT n FROM a UNION ALL SELECT n * 2::BIGINT FROM r) \
             SELECT n FROM r"
        ),
        types(&["BIGINT"])
    );
    assert_eq!(
        sql_types("SELECT g, ARRAY[g] FROM c UNION SELECT n, ARRAY[g] FROM a, c"),
        types(&["", "ARRAY<GEOMETRY>"])
    );
    assert_eq!(
        sql_types("SELECT ARRAY[n], n * 1.5 FROM a UNION SELECT ARRAY[n], n FROM b"),
        types(&["ARRAY<BIGINT>", "DECIMAL"])
    );
}
/// `(rule, message)` pairs for a query against a two-table schema.
fn diagnose(sql: &str) -> Vec<(&'static str, String)> {
    let schema = Some(load_schema(
        "CREATE TABLE a (id INT, n INT, g GEOMETRY); CREATE TABLE b (id INT, t TEXT);",
        "postgres",
    ));
    analyze_with_diagnostics(sql, "postgres", schema.as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name(), found.message))
        .collect()
}

fn rules(sql: &str) -> Vec<&'static str> {
    diagnose(sql).into_iter().map(|(rule, _)| rule).collect()
}

/// `(rule, table, column)` of each diagnostic of a query against a users/orders schema.
fn subjects(sql: &str) -> Vec<(&'static str, Option<String>, Option<String>)> {
    let schema = Some(load_schema(
        "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT NOT NULL);\
         CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT NOT NULL REFERENCES users (id),\
         status TEXT NOT NULL)",
        "postgres",
    ));
    analyze_with_diagnostics(sql, "postgres", schema.as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name(), found.table, found.column))
        .collect()
}

#[test]
fn test_name_diagnostic_subjects() {
    let some = |name: &str| Some(name.to_string());
    let cases = [
        (
            "SELECT u.nope FROM users u",
            vec![("unknown-column", some("users"), some("nope"))],
        ),
        (
            "SELECT naïve FROM users",
            vec![("unknown-column", some("users"), some("naïve"))],
        ),
        (
            "SELECT x.id FROM users u",
            vec![("unknown-table", some("x"), some("id"))],
        ),
        (
            "SELECT id FROM missing",
            vec![("unknown-table", some("missing"), None)],
        ),
        (
            "SELECT id FROM users u JOIN orders o ON o.user_id = u.id",
            vec![("ambiguous-column", None, some("id"))],
        ),
        (
            "INSERT INTO orders (id, user_id) VALUES (1, 2)",
            vec![("insert-missing-column", some("orders"), some("status"))],
        ),
        (
            "SELECT my_udf(id) FROM users",
            vec![("unknown-function", None, None)],
        ),
    ];
    for (sql, expected) in cases {
        assert_eq!(subjects(sql), expected, "{sql}");
    }
}

#[test]
fn test_name_diagnostics() {
    assert_eq!(rules("SELECT nope FROM a"), ["unknown-column"]);
    assert_eq!(rules("SELECT x.id FROM a"), ["unknown-table"]);
    assert_eq!(rules("SELECT id FROM a, b"), ["ambiguous-column"]);
    assert_eq!(rules("SELECT id FROM missing"), ["unknown-table"]);
    assert_eq!(rules("SELECT frob(n) FROM a"), ["unknown-function"]);
    assert_eq!(rules("SELECT n::mood FROM a"), ["unknown-type"]);
    assert_eq!(rules("SELECT g FROM a"), ["unknown-type"]);
    assert!(rules("SELECT id, t FROM b UNION TABLE b").is_empty());
}

#[test]
fn test_name_diagnostics_stay_quiet() {
    for sql in [
        "SELECT id FROM a JOIN b USING (id)",
        "SELECT id FROM a NATURAL JOIN b",
        "WITH c AS (SELECT n AS k FROM a) SELECT k FROM c",
        "SELECT s.k FROM (SELECT n AS k FROM a) s",
        "SELECT (SELECT max(b.id) FROM b WHERE b.id = a.id) FROM a",
        "SELECT a.id, l.m FROM a, LATERAL (SELECT max(b.id) AS m FROM b WHERE b.id = a.id) l",
        "SELECT A.ID, \"n\" FROM A",
        "SELECT NULL, $1",
    ] {
        assert!(rules(sql).is_empty(), "{sql}: {:?}", diagnose(sql));
    }
    // A table function's columns are not read, which is reported, not guessed.
    assert_eq!(
        rules("SELECT path FROM ST_Dump(NULL) AS d"),
        ["unsupported-syntax"]
    );
}

/// What each diagnostic's span covers in `sql`.
fn underlined(sql: &str) -> Vec<(&'static str, Option<&str>)> {
    let schema = Some(load_schema(
        "CREATE TABLE a (id INT, n INT, g GEOMETRY); CREATE TABLE b (id INT, t TEXT);",
        "postgres",
    ));
    analyze_with_diagnostics(sql, "postgres", schema.as_deref())
        .1
        .into_iter()
        .map(|found| (found.rule.name(), found.span.map(|span| &sql[span])))
        .collect()
}

#[test]
fn test_name_diagnostics_underline_the_name() {
    assert_eq!(
        underlined("--sql\nSELECT nope FROM a"),
        [("unknown-column", Some("nope"))]
    );
    assert_eq!(
        underlined("SELECT a.nope FROM a"),
        [("unknown-column", Some("a.nope"))]
    );
    assert_eq!(
        underlined("SELECT \"nopé\" FROM a"),
        [("unknown-column", Some("\"nopé\""))]
    );
    assert_eq!(
        underlined("SELECT id FROM a, b"),
        [("ambiguous-column", Some("id"))]
    );
    assert_eq!(
        underlined("SELECT id\nFROM a JOIN missing ON true"),
        [("unknown-table", Some("missing"))]
    );
    assert_eq!(
        underlined("SELECT upper(frob(n)) FROM a"),
        [("unknown-function", Some("frob"))]
    );
    // Inside a cast, about the cast, not the column it reads.
    assert_eq!(
        underlined("SELECT n::mood FROM a"),
        [("unknown-type", None)]
    );
    // Through the edits of the query before parsing: `%(who)s` and `%s` become `$00`.
    assert_eq!(
        underlined("SELECT %(who)s, nope FROM a"),
        [("unknown-column", Some("nope"))]
    );
    assert_eq!(
        underlined("SELECT %s, %s, nope FROM a"),
        [("unknown-column", Some("nope"))]
    );
    // PostgreSQL reads `?` placeholders only after renumbering them, which moves the names.
    assert_eq!(
        underlined("SELECT ?, nope FROM a"),
        [("unknown-column", None)]
    );
}

#[test]
fn test_name_rules_need_a_schema() {
    let found = analyze_with_diagnostics("SELECT nope, id FROM a, b", "postgres", None).1;
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn sqlite_multi_word_type_names_are_joined() {
    assert_eq!(
        sqlite_type_names(
            "CREATE TABLE t (a UNSIGNED BIG INT NOT NULL, \"b c\" VARYING CHARACTER(9), \
             d INTEGER PRIMARY KEY, CONSTRAINT k PRIMARY KEY (d), e)"
        )
        .as_deref(),
        Some(
            "CREATE TABLE t (a UNSIGNED_BIG_INT NOT NULL, \"b c\" VARYING_CHARACTER(9), \
             d INTEGER PRIMARY KEY, CONSTRAINT k PRIMARY KEY (d), e)"
        )
    );
    assert_eq!(sqlite_type_names("CREATE TABLE t (a INT NOT NULL)"), None);
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
fn rarer_star_expansions_and_result_stars() {
    let schema = Some(load_schema(
        "CREATE TABLE t (a INT); CREATE TABLE u (b INT)",
        "mysql",
    ));
    let expand = |sql: &str| expand_star(sql, "mysql", schema.as_deref());
    assert_eq!(expand("WITH c AS (SELECT 1 AS x) SELECT * FROM c, t"), None);
    assert_eq!(expand("SELECT * FROM t, (SELECT 1 AS x) s"), None);
    assert_eq!(expand("SELECT 1"), None);
    assert_eq!(
        expand("SELECT * FROM t, u").as_deref(),
        Some("SELECT t.a, u.b FROM t, u")
    );
    let stars = |sql: &str| parse(sql, "postgres").result_stars();
    assert_eq!(stars("CREATE TABLE x (a INT)"), Some(vec![]));
    assert_eq!(
        stars("(SELECT * FROM t) UNION SELECT a FROM t"),
        Some(vec!["SELECT *".to_string()])
    );
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

#[test]
fn rarer_suggestions_highlights_and_placeholders() {
    assert_eq!(
        suggest::closest("abcdefgh", &["abcdefgx", "abcdefxx"]),
        Some("abcdefgx")
    );
    assert_eq!(
        parse_error("SELECT cast(a, b)", "postgres").as_deref(),
        Some("unsupported in this dialect")
    );
    for sql in [
        "UPDATE t SET (a, b) = (1, 2)",
        "WITH x AS (SELECT 1) UPDATE t SET (a, b) = (1, 2)",
        "INSERT INTO t SET (a, b) = (1, 2)",
        "INSERT INTO t (a) VALUES (1) ON DUPLICATE KEY UPDATE (a, b) = (1, 2)",
    ] {
        assert_eq!(
            parse_error(sql, "mysql").as_deref(),
            Some("unsupported in this dialect"),
            "{sql}"
        );
    }
    assert_eq!(
        parse_error(
            "INSERT INTO t SET a = 1 ON DUPLICATE KEY UPDATE b = 2",
            "mysql"
        ),
        None
    );
    assert_eq!(
        parse_error("UPDATE t SET (a, b) = (1, 2)", "postgres"),
        None
    );
    let operators: Vec<_> = crate::highlight::highlight("SELECT a ~~~ b", "postgres")
        .into_iter()
        .map(|(range, _)| range)
        .collect();
    assert_eq!(operators, [0..6, 9..12]);
    assert_eq!(
        crate::placeholders::percent_placeholders("SELECT 5 %% 2, %s"),
        vec![15..17]
    );
}

#[test]
fn mysql_full_text_relevance_is_a_double() {
    let (schema, _) = load_schema_with_skipped(
        "CREATE TABLE t (body TEXT NOT NULL, FULLTEXT (body))",
        "mysql",
    );
    let (columns, _) = analyze_with_diagnostics(
        "SELECT MATCH(body) AGAINST('x') AS a FROM t",
        "mysql",
        Some(&schema),
    );
    assert_eq!(columns.unwrap()[0].sql_type.as_deref(), Some("DOUBLE"));
}

#[test]
fn a_result_wider_than_max_columns_is_untyped() {
    // Each CTE joins the one before to itself: 2^i + 1 columns, past the cap at c12.
    let ctes: Vec<String> = (1..=20)
        .map(|i| {
            format!(
                "c{i} AS (SELECT * FROM c{0} x JOIN c{0} y USING (a))",
                i - 1
            )
        })
        .collect();
    let sql = format!(
        "WITH c0 AS (SELECT 1 AS a, 2 AS b), {} SELECT * FROM c20",
        ctes.join(", ")
    );
    assert_eq!(names(&sql), ["*"]);
    let diagnostics = analyze_with_diagnostics(&sql, "postgres", None).1;
    let message = format!("over {} result columns are not typed", crate::MAX_COLUMNS);
    assert!(diagnostics.iter().any(|d| d.message == message));
    assert_eq!(infer_type(&sql, "postgres", None), None);
}

#[test]
fn values_and_functions_wider_than_max_columns_are_untyped() {
    let ones = vec!["1"; crate::MAX_COLUMNS + 1].join(", ");
    let sql = format!("SELECT column1 FROM (VALUES ({ones})) v");
    assert_eq!(infer_type(&sql, "postgres", None), None);
    let columns: Vec<String> = (0..=crate::MAX_COLUMNS)
        .map(|i| format!("c{i} int"))
        .collect();
    let (schema, _) = load_schema_with_skipped(
        &format!(
            "CREATE FUNCTION f() RETURNS TABLE ({}) AS 'SELECT 1' LANGUAGE sql;",
            columns.join(", ")
        ),
        "postgres",
    );
    assert_eq!(
        infer_type("SELECT c0 FROM f()", "postgres", Some(&schema)),
        None
    );
}

#[test]
fn stars_over_wide_tables_stop_past_max_columns() {
    let columns: Vec<String> = (0..crate::MAX_COLUMNS)
        .map(|i| format!("c{i} int"))
        .collect();
    let schema = format!("CREATE TABLE big ({});", columns.join(", "));
    let (schema, _) = load_schema_with_skipped(&schema, "postgres");
    let message = format!("over {} result columns are not typed", crate::MAX_COLUMNS);
    for sql in [
        "SELECT * FROM big b0, big b1, big b2",
        "SELECT b0.*, b1.*, b2.* FROM big b0, big b1, big b2",
        "SELECT * FROM big b0 JOIN big b1 USING (c0) JOIN big b2 USING (c0), big b3",
        "UPDATE big SET c0 = 1 FROM big b0, big b1 RETURNING *",
    ] {
        assert_eq!(infer_type(sql, "postgres", Some(&schema)), None, "{sql}");
        let diagnostics = analyze_with_diagnostics(sql, "postgres", Some(&schema)).1;
        assert!(diagnostics.iter().any(|d| d.message == message), "{sql}");
    }
}

#[test]
fn a_star_over_more_than_64_merging_joins_is_untyped() {
    let schema = load_schema_with_skipped("CREATE TABLE t (a int NOT NULL);", "postgres").0;
    let natural = |n: usize| {
        let joins: String = (1..n).map(|i| format!(" NATURAL JOIN t t{i}")).collect();
        format!("SELECT * FROM (t t0{joins})")
    };
    assert_eq!(
        infer_type(&natural(64), "postgres", Some(&schema)).as_deref(),
        Some("tuple[int]")
    );
    assert_eq!(infer_type(&natural(65), "postgres", Some(&schema)), None);
}

#[test]
fn update_narrowing_folds_unquoted_set_columns() {
    let schema = load_schema_with_skipped("CREATE TABLE t (name TEXT);", "postgres").0;
    let infer = |sql: &str| infer_type(sql, "postgres", Some(&schema));
    assert_eq!(
        infer("UPDATE t SET Name = NULL WHERE name IS NOT NULL RETURNING name").as_deref(),
        Some("tuple[str | None]")
    );
    assert_eq!(
        infer("UPDATE t SET NAME = 'x' RETURNING name").as_deref(),
        Some("tuple[str]")
    );
}

#[test]
fn reads_of_a_deprecated_column_are_reported_writes_are_not() {
    let types = [
        deprecated_type("a.n", Some("id"), Some("2027-01-01")),
        deprecated_type("public.b.t", None, None),
    ];
    let overrides = TypeOverrides::new(&types, "postgres").unwrap();
    let schema = "CREATE TABLE a (id INT, n INT, g GEOMETRY); CREATE TABLE b (id INT, t TEXT);";
    let (schema, _) = load_schema_with_overrides(schema, "postgres", overrides);
    let found = |sql: &str| -> Vec<(String, Option<String>)> {
        analyze_with_diagnostics(sql, "postgres", Some(&schema))
            .1
            .into_iter()
            .map(|found| (found.message, found.table))
            .collect()
    };
    let n = "`a.n` is deprecated: read `id` instead, it is removed after 2027-01-01";
    assert_eq!(
        found("SELECT n FROM a"),
        [(n.to_string(), Some("a".into()))]
    );
    assert_eq!(
        found("SELECT b.t, g FROM a JOIN b USING (id) WHERE n > 0"),
        [
            (n.to_string(), Some("a".into())),
            ("`b.t` is deprecated".to_string(), Some("b".into())),
            (
                "column `a.g` has type `GEOMETRY`, which has no Python type".to_string(),
                None
            ),
        ]
    );
    assert_eq!(found("UPDATE a SET n = 1 RETURNING id"), []);
    assert_eq!(found("INSERT INTO b (id, t) VALUES (1, 'x')"), []);
    assert!(!TypeOverrides::new(&types, "postgres").unwrap().is_empty());
}

#[test]
fn tenant_scope_reports_statements_that_do_not_pin_the_tenant_key() {
    let schema = "CREATE TABLE orgs (id SERIAL PRIMARY KEY, parent_id INT REFERENCES orgs, name TEXT);
        CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INT NOT NULL REFERENCES orgs, name TEXT);
        CREATE TABLE invoices (id SERIAL PRIMARY KEY, org_id INT NOT NULL REFERENCES orgs, project_id INT, name TEXT);
        CREATE TABLE audit_log (id SERIAL PRIMARY KEY, org_id INT REFERENCES orgs);
        CREATE TABLE events (id SERIAL PRIMARY KEY, org INT NOT NULL);
        CREATE TABLE transfers (id SERIAL PRIMARY KEY, a INT REFERENCES orgs, b INT REFERENCES orgs);
        CREATE TABLE secrets (id SERIAL PRIMARY KEY, org_id INT REFERENCES orgs);
        CREATE TABLE tags (id SERIAL PRIMARY KEY, name TEXT);
        ALTER TABLE secrets ENABLE ROW LEVEL SECURITY;
        ALTER TABLE secrets FORCE ROW LEVEL SECURITY;
        CREATE POLICY tenant ON secrets USING (org_id = current_setting('app.org_id')::int);
        CREATE TABLE regions (id INT, region TEXT, PRIMARY KEY (id, region));
        CREATE TABLE sites (id INT, org_id INT, region TEXT, FOREIGN KEY (org_id, region) REFERENCES regions);
        CREATE VIEW names AS SELECT name FROM projects;";
    let load = |table: &str| {
        let types = [tenancy_type(
            table,
            &["audit_log.org_id"],
            &["public.events.org"],
            &[],
        )];
        let overrides = TypeOverrides::new(&types, "postgres").unwrap();
        assert!(!overrides.is_empty());
        load_schema_with_overrides(schema, "postgres", overrides).0
    };
    let (orgs, regions) = (load("public.orgs"), load("regions"));
    let found = |sql: &str, schema: &Schema| -> Vec<String> {
        analyze_with_diagnostics(sql, "postgres", Some(schema))
            .1
            .into_iter()
            .filter(|found| found.rule == crate::diagnostics::Rule::TenantScope)
            .map(|found| found.message)
            .collect()
    };
    let read = |table: &str, column: &str| {
        vec![format!(
            "`{table}` is read without `{table}.{column} = <parameter>`"
        )]
    };
    for sql in [
        "SELECT name FROM projects WHERE org_id = %s",
        "SELECT name FROM projects WHERE org_id IN ($1, $2)",
        "SELECT name FROM projects WHERE org_id = ANY($1)",
        "SELECT name FROM projects WHERE $1::INT = org_id",
        "SELECT name FROM projects WHERE org_id IS NOT DISTINCT FROM $1",
        "SELECT p.name FROM orgs o JOIN projects p ON p.org_id = o.id WHERE o.id = $1",
        "SELECT p.name FROM orgs o, projects p WHERE p.org_id = o.id AND o.id = $1",
        "SELECT i.id FROM invoices i JOIN projects p USING (org_id) WHERE p.org_id = $1",
        "SELECT i.id FROM projects p LEFT JOIN invoices i ON i.org_id = p.org_id WHERE p.org_id = $1",
        "SELECT i.id FROM (projects p JOIN invoices i ON i.org_id = p.org_id) WHERE p.org_id = $1",
        "SELECT p.id FROM invoices i RIGHT JOIN projects p ON i.org_id = p.org_id WHERE p.org_id = $1",
        "SELECT p.id FROM projects p WHERE p.org_id = $1 AND EXISTS (SELECT 1 FROM invoices i WHERE i.org_id = p.org_id)",
        "SELECT id FROM audit_log",
        "SELECT id FROM secrets",
        "SELECT id FROM events WHERE org = $1",
        "SELECT id FROM transfers WHERE b = $1",
        "SELECT name FROM names",
        "SELECT x FROM generate_series(1, 2) x",
        "WITH projects AS (SELECT 1 AS id) SELECT id FROM projects",
        "UPDATE projects SET name = $1, org_id = $2 WHERE org_id = $3",
        "DELETE FROM invoices USING projects p WHERE invoices.org_id = p.org_id AND p.org_id = $1",
        "INSERT INTO projects (org_id, name) VALUES ($1, $2)",
        "INSERT INTO projects VALUES (DEFAULT, $1, $2)",
        "INSERT INTO orgs (name) VALUES ($1)",
        "INSERT INTO invoices (org_id, project_id) SELECT org_id, id FROM projects WHERE org_id = $1",
        "INSERT INTO projects (id, org_id) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET org_id = EXCLUDED.org_id",
        "INSERT INTO projects (id, org_id) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET name = 'x', (org_id) = (1)",
        "INSERT INTO projects (id, org_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        "UPDATE (SELECT 1) d SET x = 1",
        "UPDATE tags SET name = $1",
        "SELECT i.id FROM projects p LEFT JOIN invoices i ON i.org_id = $1 WHERE p.org_id = $1",
        "SELECT i.id FROM projects p LEFT JOIN invoices i ON p.org_id = $1 AND i.org_id = $1 WHERE p.org_id = $1",
        "SELECT p.id FROM projects p JOIN invoices i USING (org_id) JOIN audit_log a USING (id) WHERE p.org_id = $1",
        "SELECT p.id FROM projects p JOIN invoices i USING (name) WHERE p.org_id = $1 AND i.org_id = $1",
        "SELECT name FROM projects p WHERE org_id = $1 AND p.org_id = (SELECT 1)",
        "SELECT p.id FROM projects p JOIN invoices i USING (nothing) WHERE p.org_id = $1 AND i.org_id = $1",
        "SELECT a.id FROM projects a JOIN projects b ON b.org_id = a.org_id JOIN projects c ON c.org_id = b.org_id AND c.org_id = a.org_id WHERE a.org_id = $1",
        "INSERT INTO audit_log (org_id) VALUES (1)",
        "INSERT INTO names (name) VALUES ($1)",
        "MERGE INTO projects p USING invoices i ON p.id = i.id WHEN MATCHED THEN DELETE",
        "SELECT i.id FROM projects p JOIN (invoices i JOIN audit_log a ON a.id = i.id) ON i.org_id = p.org_id WHERE p.org_id = $1",
        "SELECT p.id FROM projects p JOIN invoices i ON true WHERE p.org_id = $1 AND i.org_id = p.org_id",
        "SELECT p.id FROM projects p NATURAL JOIN invoices i WHERE p.org_id = $1 AND i.org_id = $1",
        "SELECT p.id FROM projects p JOIN LATERAL (SELECT 1 AS one) l ON true WHERE p.org_id = $1",
        "SELECT p.id FROM projects p JOIN invoices i USING (id) WHERE p.org_id = $1 AND i.org_id = $1",
        "SELECT p.id FROM projects p, (SELECT 1 AS id) d WHERE p.org_id = $1",
        "UPDATE ONLY projects SET name = $1 WHERE org_id = $2",
        "DELETE FROM projects WHERE org_id = $1",
        "SELECT name FROM projects WHERE org_id = $1 AND name = $2 OR id = $3 AND org_id = $1 AND false",
        "WITH p AS (SELECT name, org_id FROM projects) SELECT name FROM p WHERE org_id = $1",
        "WITH p AS (SELECT name FROM projects WHERE org_id = $1) SELECT name FROM p",
        "SELECT d.name FROM (SELECT * FROM projects) d WHERE d.org_id = $1",
        "SELECT d.name FROM (SELECT p.* FROM projects p) d WHERE d.org_id = $1",
        "WITH p (n, t) AS (SELECT name, org_id FROM projects) SELECT n FROM p WHERE t = $1",
        "WITH a AS (SELECT name, org_id FROM projects), b AS (SELECT name, org_id FROM a) SELECT name FROM b WHERE org_id = $1",
        "WITH u AS (SELECT id, org_id FROM projects UNION ALL (SELECT id, org_id FROM invoices)) SELECT id FROM u WHERE org_id = $1",
        "WITH p AS (SELECT id, org_id FROM projects WHERE org_id = $1) SELECT i.id FROM invoices i JOIN p ON p.org_id = i.org_id",
        "SELECT n FROM (SELECT org_id, count(*)::INT AS n FROM projects GROUP BY org_id) c WHERE c.org_id = $1",
        "WITH p AS (SELECT i.id, p.org_id FROM projects p JOIN invoices i ON i.org_id = p.org_id) SELECT id FROM p WHERE org_id = $1",
        "WITH p AS (SELECT id, org_id FROM projects) SELECT i.id FROM invoices i WHERE i.org_id = $1 AND EXISTS (SELECT 1 FROM p WHERE p.org_id = i.org_id)",
        "WITH p AS (SELECT id, org_id FROM projects) UPDATE invoices SET project_id = p.id FROM p WHERE p.org_id = invoices.org_id AND invoices.org_id = $1",
        "WITH p AS (SELECT id, org_id FROM projects) DELETE FROM invoices WHERE org_id = $1 AND project_id IN (SELECT id FROM p WHERE org_id = $1)",
        "WITH t AS (SELECT 1 AS x, 2 AS x) SELECT 1 FROM t",
        "SELECT g FROM (SELECT x AS g FROM generate_series(1, 2) x) d",
        "SELECT x FROM (VALUES (1)) v (x)",
        "WITH p AS (SELECT name, org_id FROM projects) SELECT name FROM p WHERE org_id = $1 AND EXISTS (WITH p AS (SELECT 1 AS one) SELECT one FROM p)",
        "CREATE TABLE copy (id INT)",
    ] {
        let expected: Vec<String> = match sql {
            s if s.ends_with("false") => read("projects", "org_id"),
            _ => Vec::new(),
        };
        assert_eq!(found(sql, &orgs), expected, "{sql}");
    }
    for (sql, expected) in [
        ("SELECT name FROM orgs", read("orgs", "id")),
        ("SELECT name FROM orgs WHERE parent_id = $1", read("orgs", "id")),
        ("SELECT name FROM projects WHERE org_id = 1", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE org_id > $1", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE org_id IN ($1, 2)", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE org_id NOT IN ($1)", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE org_id <> ALL($1)", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE org_id = name", read("projects", "org_id")),
        ("INSERT INTO projects (org_id) TABLE projects", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE lower(name) = $1", read("projects", "org_id")),
        ("SELECT name FROM projects WHERE org_id = $1 OR id = $2", read("projects", "org_id")),
        (
            "SELECT i.id FROM projects p LEFT JOIN invoices i ON i.org_id = p.org_id AND i.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT i.id FROM projects p FULL JOIN invoices i ON i.org_id = p.org_id WHERE p.org_id = $1",
            read("invoices", "org_id"),
        ),
        (
            "SELECT i.id FROM projects p RIGHT JOIN invoices i ON i.org_id = p.org_id AND p.org_id = $1",
            read("invoices", "org_id"),
        ),
        (
            "SELECT p.id FROM projects p WHERE p.org_id = $1 AND p.id IN (SELECT project_id FROM invoices i WHERE i.project_id = p.id)",
            read("invoices", "org_id"),
        ),
        (
            "SELECT id FROM transfers",
            vec!["`transfers` is read without `transfers.a = <parameter>` or `transfers.b = <parameter>`".into()],
        ),
        (
            "UPDATE projects SET name = $1 WHERE id = $2",
            vec!["`projects` is updated without `projects.org_id = <parameter>`".into()],
        ),
        (
            "UPDATE projects SET org_id = 2 WHERE org_id = $1",
            vec!["UPDATE of `projects` does not set `org_id` from a parameter".into()],
        ),
        (
            "UPDATE invoices SET project_id = p.id FROM projects p WHERE invoices.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "DELETE FROM events WHERE id < $1",
            vec!["`events` is deleted from without `events.org = <parameter>`".into()],
        ),
        (
            "INSERT INTO projects (org_id, name) VALUES ($1, $2), (2, $3)",
            vec!["INSERT into `projects` does not set `org_id` from a parameter".into()],
        ),
        (
            "INSERT INTO projects (name) VALUES ($1)",
            vec!["INSERT into `projects` does not set `org_id` from a parameter".into()],
        ),
        (
            "INSERT INTO invoices (org_id, project_id) SELECT 1, id FROM projects WHERE org_id = $1",
            vec!["INSERT into `invoices` does not set `org_id` from a parameter".into()],
        ),
        (
            "INSERT INTO projects (id, org_id) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET org_id = 1",
            vec!["UPDATE of `projects` does not set `org_id` from a parameter".into()],
        ),
        ("WITH p AS (SELECT name, org_id FROM projects) SELECT name FROM p", read("projects", "org_id")),
        (
            "WITH p AS (SELECT name, id FROM projects) SELECT p.name FROM p JOIN invoices i ON i.project_id = p.id WHERE i.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "WITH u AS (SELECT id, org_id FROM projects UNION ALL SELECT id, 1 FROM invoices) SELECT id FROM u WHERE org_id = $1",
            read("invoices", "org_id"),
        ),
        (
            "WITH u AS (SELECT id, org_id FROM projects UNION ALL SELECT id FROM invoices) SELECT id FROM u WHERE org_id = $1",
            [read("invoices", "org_id"), read("projects", "org_id")].concat(),
        ),
        (
            "WITH x AS (SELECT p.name, p.org_id, i.org_id FROM projects p, invoices i WHERE p.org_id = $1) SELECT name FROM x",
            read("invoices", "org_id"),
        ),
        (
            "WITH RECURSIVE r AS (SELECT id, org_id FROM projects UNION ALL SELECT id, org_id FROM r) SELECT id FROM r WHERE org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT name FROM (SELECT name, org_id, row_number() OVER () AS n FROM projects) r WHERE r.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "WITH p AS (SELECT name, org_id FROM projects LIMIT 10) SELECT name FROM p WHERE org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT n FROM (SELECT max(org_id) AS org_id, count(*)::INT AS n FROM projects GROUP BY name) c WHERE c.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT n FROM (SELECT org_id, count(*)::INT AS n FROM projects GROUP BY ALL) c WHERE c.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT name FROM (SELECT DISTINCT ON (name) name, org_id FROM projects) d WHERE d.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT name FROM (SELECT * FROM projects JOIN invoices USING (id)) d WHERE d.org_id = $1",
            [read("projects", "org_id"), read("invoices", "org_id")].concat(),
        ),
        (
            "SELECT name FROM (SELECT d.* FROM (SELECT name, org_id, row_number() OVER () AS n FROM projects) d) e WHERE e.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT name FROM (SELECT (p).* FROM projects p) d WHERE d.org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "SELECT name FROM (SELECT name, org_id FROM projects) WHERE org_id = $1",
            read("projects", "org_id"),
        ),
        (
            "WITH p AS (SELECT id, org_id FROM projects) MERGE INTO invoices i USING p ON i.project_id = p.id WHEN MATCHED THEN DELETE",
            read("projects", "org_id"),
        ),
        (
            "MERGE INTO invoices i USING (SELECT id, org_id FROM projects) p ON i.project_id = p.id WHEN MATCHED THEN DELETE",
            read("projects", "org_id"),
        ),
    ] {
        assert_eq!(found(sql, &orgs), expected, "{sql}");
    }
    let both =
        "`sites` is read without `sites.org_id = <parameter>` and `sites.region = <parameter>`";
    assert_eq!(
        found("SELECT id FROM sites WHERE region = $1", &regions),
        [both]
    );
    assert_eq!(
        found(
            "SELECT id FROM sites WHERE org_id = $1 AND region = $2",
            &regions
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        found(
            "WITH s AS (SELECT id, org_id FROM sites WHERE region = $1) SELECT id FROM s WHERE org_id = $2",
            &regions
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        found(
            "SELECT name FROM projects",
            &load_schema(schema, "postgres")
        ),
        Vec::<String>::new()
    );
}

#[test]
fn tenant_scope_reads_mysql_joins_and_assignments() {
    let types = [tenancy_type("orgs", &[], &[], &[])];
    let overrides = TypeOverrides::new(&types, "mysql").unwrap();
    let schema = "CREATE TABLE orgs (id INT PRIMARY KEY);
        CREATE TABLE projects (id INT PRIMARY KEY, org_id INT NOT NULL, FOREIGN KEY (org_id) REFERENCES orgs (id));
        CREATE TABLE invoices (id INT PRIMARY KEY, org_id INT NOT NULL, FOREIGN KEY (org_id) REFERENCES orgs (id));";
    let (schema, _) = load_schema_with_overrides(schema, "mysql", overrides);
    let found = |sql: &str| -> Vec<String> {
        analyze_with_diagnostics(sql, "mysql", Some(&schema))
            .1
            .into_iter()
            .filter(|found| found.rule == crate::diagnostics::Rule::TenantScope)
            .map(|found| found.message)
            .collect()
    };
    assert_eq!(
        found("SELECT i.id FROM projects p STRAIGHT_JOIN invoices i ON i.org_id = p.org_id WHERE p.org_id = ?"),
        Vec::<String>::new()
    );
    assert_eq!(
        found("INSERT INTO projects SET id = ?, org_id = ?"),
        Vec::<String>::new()
    );
    assert_eq!(
        found("INSERT INTO FUNCTION remote('x') VALUES (1)"),
        Vec::<String>::new()
    );
    assert_eq!(
        found("INSERT INTO projects SET id = ?, org_id = 1"),
        ["INSERT into `projects` does not set `org_id` from a parameter"]
    );
    assert_eq!(
        found("UPDATE projects p JOIN invoices i ON i.org_id = p.org_id SET p.id = ? WHERE p.org_id = ?"),
        Vec::<String>::new()
    );
}

#[test]
fn tenant_scope_reads_row_level_security_policies() {
    let rls = |name: &str, policies: &str| {
        format!(
            "CREATE TABLE {name} (id INT PRIMARY KEY, org_id INT, owner TEXT);
            ALTER TABLE {name} ENABLE ROW LEVEL SECURITY;
            ALTER TABLE {name} FORCE ROW LEVEL SECURITY;
            {policies}"
        )
    };
    let tenant = "current_setting('app.org_id')::int";
    let schema = [
        rls("per_command", &format!(
            "CREATE POLICY s ON per_command FOR SELECT USING (org_id = {tenant});
            CREATE POLICY i ON per_command FOR INSERT WITH CHECK ({tenant} = org_id);
            CREATE POLICY u ON per_command FOR UPDATE USING (org_id = {tenant}) WITH CHECK (org_id = {tenant});
            CREATE POLICY d ON per_command FOR DELETE USING ((org_id) = ({tenant}));"
        )),
        rls("no_delete", &format!("CREATE POLICY s ON no_delete FOR SELECT USING (org_id = {tenant});")),
        rls("loose_update", &format!(
            "CREATE POLICY a ON loose_update USING (org_id = {tenant});
            CREATE POLICY u ON loose_update FOR UPDATE USING (org_id = {tenant}) WITH CHECK (true);"
        )),
        rls("for_admins", &format!(
            "CREATE POLICY a ON for_admins USING (true);
            CREATE POLICY r ON for_admins AS RESTRICTIVE TO admin USING (org_id = {tenant});"
        )),
        rls("users", "CREATE POLICY a ON users AS PERMISSIVE TO PUBLIC USING (owner = session_user AND id > 0 AND owner::text = current_user);"),
        rls("renamed", &format!(
            "CREATE POLICY a ON renamed USING (org_id = {tenant});
            ALTER POLICY a ON renamed RENAME TO b;
            ALTER POLICY b ON renamed TO admin;
            ALTER POLICY missing ON renamed USING (true);
            DROP POLICY a ON renamed;"
        )),
        rls("restored", &format!(
            "CREATE POLICY a ON restored TO admin USING (org_id = {tenant});
            ALTER POLICY a ON restored TO public;"
        )),
        rls("dropped", &format!(
            "CREATE POLICY a ON dropped USING (org_id = {tenant});
            CREATE POLICY b ON dropped USING (true);
            DROP POLICY a ON dropped;"
        )),
        rls("unforced", &format!(
            "CREATE POLICY a ON unforced USING (org_id = {tenant});
            ALTER TABLE unforced NO FORCE ROW LEVEL SECURITY;"
        )),
        rls("lost_column", &format!(
            "CREATE POLICY a ON lost_column USING (org_id = {tenant});
            CREATE POLICY b ON lost_column USING (owner = (SELECT 1, 2));
            CREATE POLICY c ON lost_column USING (owner = (SELECT current_user UNION SELECT 'x'));
            CREATE POLICY d ON lost_column USING (owner = lower('x') OR id = id);
            ALTER TABLE lost_column DROP COLUMN org_id;"
        )),
        rls("supabase", "CREATE POLICY a ON supabase USING (owner = (SELECT auth.uid()) AND org_id = (SELECT 1) AND id = 'x' AND org_id = (VALUES (1)));
            ALTER TABLE supabase RENAME COLUMN owner TO user_id;
            ALTER TABLE supabase RENAME COLUMN id TO key;"),
        rls("unread", &format!(
            "CREATE POLICY a ON unread USING (org_id = {tenant});
            CREATE POLICY b ON unread USING (id = ANY (SELECT 1 WINDOW w AS ()));"
        )),
        rls("unread_alter", &format!(
            "CREATE POLICY a ON unread_alter USING (unread_alter.org_id = {tenant});
            ALTER POLICY a ON unread_alter USING (id = ANY (SELECT 1 WINDOW w AS ()));"
        )),
        rls("quoted", &format!(
            "CREATE POLICY a ON quoted USING (quoted.org_id = {tenant} AND owner = (SELECT auth.uid() AS uid));
            ALTER TABLE quoted RENAME COLUMN org_id TO \"OrgId\";
            ALTER TABLE quoted NO FORCE ROW LEVEL SECURITY;"
        )),
        "CREATE TABLE soft (id INT, owner TEXT);
            CREATE POLICY a ON soft USING (owner = auth.uid()::text);".to_string(),
    ]
    .join("\n");
    let types = [tenancy_type("", &[], &["dropped.org_id"], &["Auth.UID"])];
    let overrides = TypeOverrides::new(&types, "postgres").unwrap();
    let sql = schema;
    let (schema, _) = load_schema_with_overrides(&sql, "postgres", overrides);
    let policies: Vec<(String, String)> = schema
        .tenant_policies()
        .into_iter()
        .map(|found| {
            let words: Vec<&str> = sql[found.span].split_whitespace().take(5).collect();
            (found.message, words.join(" "))
        })
        .collect();
    let gap =
        |policy: &str, table: &str, what: &str| format!("policy `{policy}` on `{table}` {what}");
    let check = "; compare it to the session in WITH CHECK";
    let filter = "; compare it to the session in USING";
    assert_eq!(
        policies,
        [
            (gap("u", "loose_update", &format!("does not check `org_id` on UPDATE{check}")), "CREATE POLICY u ON loose_update"),
            (gap("a", "for_admins", &format!("does not filter SELECT, UPDATE, DELETE or check INSERT by `org_id`{filter}")), "CREATE POLICY a ON for_admins"),
            (gap("b", "dropped", &format!("does not filter SELECT, UPDATE, DELETE or check INSERT by `org_id`{filter}")), "CREATE POLICY b ON dropped"),
            ("`unforced` has row-level security without `FORCE ROW LEVEL SECURITY`, so its owner sees every tenant".to_string(), "ALTER TABLE unforced NO FORCE"),
            ("sqlty can't read this policy on `unread`, so it counts as letting every tenant's rows through".to_string(), "CREATE POLICY b ON unread"),
            ("sqlty can't read this policy on `unread_alter`, so it counts as letting every tenant's rows through".to_string(), "ALTER POLICY a ON unread_alter"),
            ("`quoted` has row-level security without `FORCE ROW LEVEL SECURITY`, so its owner sees every tenant".to_string(), "ALTER TABLE quoted NO FORCE"),
        ]
        .map(|(message, at)| (message, at.to_string()))
    );
    let found = |sql: &str| -> Vec<String> {
        analyze_with_diagnostics(sql, "postgres", Some(&schema))
            .1
            .into_iter()
            .filter(|found| found.rule == crate::diagnostics::Rule::TenantScope)
            .map(|found| found.message)
            .collect()
    };
    let read = |table: &str, column: &str| {
        vec![format!(
            "`{table}` is read without `{table}.{column} = <parameter>`"
        )]
    };
    assert_eq!(found("SELECT id FROM per_command"), Vec::<String>::new());
    assert_eq!(found("SELECT id FROM no_delete"), Vec::<String>::new());
    assert_eq!(found("SELECT id FROM restored"), Vec::<String>::new());
    assert_eq!(found("SELECT id FROM lost_column"), Vec::<String>::new());
    assert_eq!(
        found("SELECT id FROM loose_update"),
        read("loose_update", "org_id")
    );
    assert_eq!(
        found("SELECT id FROM for_admins"),
        read("for_admins", "org_id")
    );
    assert_eq!(found("SELECT id FROM renamed"), Vec::<String>::new());
    assert_eq!(found("SELECT id FROM dropped"), read("dropped", "org_id"));
    assert_eq!(found("SELECT id FROM unforced"), read("unforced", "org_id"));
    assert_eq!(found("SELECT id FROM users"), Vec::<String>::new());
    assert_eq!(found("DELETE FROM users"), Vec::<String>::new());
    assert_eq!(found("SELECT key FROM supabase"), Vec::<String>::new());
    assert_eq!(found("SELECT id FROM soft"), read("soft", "owner"));
    assert_eq!(found("SELECT id FROM unread"), read("unread", "org_id"));
    assert_eq!(
        found("SELECT id FROM unread_alter"),
        read("unread_alter", "org_id")
    );
    assert_eq!(
        found("SELECT id FROM quoted"),
        ["`quoted` is read without `quoted.OrgId = <parameter>` or `quoted.owner = <parameter>`"]
    );
}
