//! Analyzer unit tests that cross module boundaries.

use super::input::parser_input;
use super::preprocess::{
    numbered_placeholders, parse_first, parse_query, sqlite_type_names, PARSES,
};
use super::query::Analyzer;
use super::*;

fn infer(sql: &str) -> Option<String> {
    infer_type(sql, "postgres", None).unwrap()
}

#[test]
fn sum_of_a_column_missing_from_the_schema_is_an_error() {
    let (schema, _) = load_schema_with_skipped("CREATE TABLE users (id INT)", "postgres");
    let error = infer_type("SELECT SUM(nope) FROM users", "postgres", Some(&schema)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Column 'nope' not found in table 'users' in schema. If you want to query \
         columns not in the schema, use an explicit cast: SUM(nope::TYPE)"
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
        infer_type("SELECT * FROM users", "postgres", schema.as_deref())
            .unwrap()
            .as_deref(),
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
        "SELECT NULL, NULL, NULL, NULL, '%s', 5 % 2"
    );
    assert_eq!(
        parser_input("SELECT '100%%', 'it\\'s %s', %s", SqlDialect::MySql).text(),
        "SELECT '100%', 'it\\'s %s', NULL"
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
        parser_input(
            "INSERT INTO t (a) VALUES (1) ON CONFLICT (a, lower(b)) WHERE c = ')DO(' OR d = 'x''DO' DO NOTHING",
            SqlDialect::Postgres
        )
        .text(),
        "INSERT INTO t (a) VALUES (1) ON CONFLICT (a, lower(b)) DO NOTHING"
    );
    assert_eq!(
        parser_input("SELECT 1 ON CONFLICT (a) WHERE b", SqlDialect::Postgres).text(),
        "SELECT 1 ON CONFLICT (a) WHERE b"
    );
    assert_eq!(
        parser_input(
            "SELECT CIDR '10.0.0.0/8', inet 'it''s', \"name\" 'x', 'oid ''1''' -- money '1'",
            SqlDialect::Postgres
        ).text(),
        "SELECT CAST('10.0.0.0/8' AS CIDR), CAST('it''s' AS inet), \"name\" 'x', 'oid ''1''' -- money '1'"
    );
    assert_eq!(
        parser_input("SELECT inet '1.2.3.4'", SqlDialect::MySql).text(),
        "SELECT inet '1.2.3.4'"
    );
    assert_eq!(
        infer_type("--sql\nSELECT 1 AS x", "mysql", None)
            .unwrap()
            .as_deref(),
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
fn test_sum_of_unknown_schema_column_is_an_error() {
    let schema = Some(load_schema("CREATE TABLE users (id INT)", "postgres"));
    assert!(infer_type("SELECT SUM(nope) FROM users", "postgres", schema.as_deref()).is_err());
}

#[test]
fn test_ctes_and_derived_tables() {
    let schema = Some(load_schema(
        "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, born DATE NOT NULL)",
        "postgres",
    ));
    let infer = |sql: &str| infer_type(sql, "postgres", schema.as_deref()).unwrap();
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
