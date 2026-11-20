//! The analyzer's entry points besides the per-query reads of `diagnostics.rs`.

use sqlty_sql::analyzer::{self, TypeOverrides};

#[test]
fn statements_split_on_semicolons_outside_quotes() {
    let sql = "SELECT 1; -- two\nSELECT 'a;b';";
    let ranges = analyzer::statement_ranges(sql, "mysql");
    let texts: Vec<&str> = ranges.iter().map(|range| &sql[range.clone()]).collect();
    assert_eq!(texts, ["SELECT 1", "SELECT 'a;b'"]);
}

#[test]
fn only_queries_return_rows() {
    assert!(analyzer::returns_rows("SELECT 1", "postgres"));
    assert!(analyzer::returns_rows("SELEC nope", "postgres"));
    assert!(!analyzer::returns_rows("DELETE FROM t", "postgres"));
    for sql in [
        "SET CONSTRAINTS a, \"b\".c DEFERRED",
        "DO $$ BEGIN END $$",
        "DO LANGUAGE plpgsql 'BEGIN END'",
        "DO 'BEGIN END' LANGUAGE plpgsql",
    ] {
        assert!(!analyzer::returns_rows(sql, "postgres"), "{sql}");
    }
}

#[test]
fn parse_errors_say_why() {
    assert_eq!(analyzer::parse_error("SELECT 1", "postgres"), None);
    let error = analyzer::parse_error("SELECT FROM WHERE (", "postgres").unwrap_or_default();
    assert!(error.starts_with("Expected"), "{error}");
    let deep = format!("SELECT 1{}", " + 1".repeat(1100));
    let error = analyzer::parse_error(&deep, "postgres").unwrap_or_default();
    assert!(error.contains("nest"), "{error}");
}

#[test]
fn a_composite_type_lists_its_fields() {
    let schema =
        analyzer::load_schema_with_skipped("CREATE TYPE pair AS (a INT, b TEXT)", "postgres").0;
    let fields = analyzer::composite_columns("PAIR", "postgres", &schema).unwrap_or_default();
    let fields: Vec<String> = fields
        .iter()
        .map(|c| format!("{}: {}", c.name, c.display_type().unwrap_or_default()))
        .collect();
    assert_eq!(fields, ["a: int | None", "b: str | None"]);
    assert!(analyzer::composite_columns("nope", "postgres", &schema).is_none());
}

#[test]
fn type_overrides_name_their_modules() {
    let types = [
        ("DATE".to_string(), "pendulum.Date".to_string()),
        ("t.a".to_string(), "money.types.Money | None".to_string()),
    ];
    let overrides = TypeOverrides::new(&types, "postgres").unwrap_or_else(|e| panic!("{e}"));
    let schema =
        analyzer::load_schema_with_overrides("CREATE TABLE t (a INT)", "postgres", overrides).0;
    assert_eq!(schema.type_modules(), ["money.types", "pendulum"]);
    let bad = [("NOT A TYPE(".to_string(), "int".to_string())];
    let error = TypeOverrides::new(&bad, "postgres")
        .err()
        .unwrap_or_default();
    assert_eq!(error, "`NOT A TYPE(` is not a SQL type");
}

#[test]
fn python_types_are_those_columns_take() {
    let types = analyzer::python_types();
    for python in ["int", "str", "decimal.Decimal", "typing.Any"] {
        assert!(types.contains(python), "{python}");
    }
}

#[test]
fn result_stars_list_the_stars_that_shape_rows() {
    let stars = |sql: &str| analyzer::parse(sql, "postgres").result_stars();
    assert_eq!(
        stars("WITH c AS (SELECT * FROM t) SELECT c.* FROM c UNION (SELECT * FROM u)"),
        Some(vec![
            "WITH c AS (SELECT *)".to_string(),
            "SELECT c.*".to_string(),
            "SELECT *".to_string(),
        ])
    );
    assert_eq!(
        stars("INSERT INTO t SELECT 1 RETURNING *"),
        Some(vec!["RETURNING *".to_string()])
    );
    assert_eq!(
        stars("UPDATE t SET a = 1 RETURNING t.*"),
        Some(vec!["RETURNING t.*".to_string()])
    );
    assert_eq!(
        stars("WITH c AS (SELECT 1) DELETE FROM t RETURNING *"),
        Some(vec!["RETURNING *".to_string()])
    );
    assert_eq!(stars("VALUES (1)"), Some(vec![]));
    assert_eq!(stars("CREATE TABLE t (a INT)"), Some(vec![]));
    assert_eq!(stars("SELEC"), None);
}

#[test]
fn expand_star_spells_out_the_columns() {
    let schema = analyzer::load_schema_with_skipped(
        "CREATE TABLE t (a INT NOT NULL, b TEXT); CREATE TABLE u (a INT)",
        "postgres",
    )
    .0;
    let expand = |sql: &str| analyzer::expand_star(sql, "postgres", Some(&schema));
    assert_eq!(
        expand("SELECT coalesce(t.b, 'x') AS c, t.* FROM t"),
        Some("SELECT coalesce(t.b, 'x') AS c, t.a::INT, t.b::TEXT FROM t".to_string())
    );
    assert_eq!(expand("WITH t AS (SELECT 1 AS z) SELECT * FROM t"), None);
    assert_eq!(
        expand("WITH c AS (SELECT 1 AS z) SELECT c.* FROM t, c"),
        None
    );
    assert_eq!(expand("WITH c AS (SELECT 1 AS z) SELECT * FROM t, c"), None);
    assert_eq!(
        expand("SELECT u.* FROM generate_series(1, 2) g, u"),
        Some("SELECT u.a::INT FROM generate_series(1, 2) g, u".to_string())
    );
    assert_eq!(expand("DELETE FROM t"), None);
    assert_eq!(
        analyzer::expand_star("SELECT * FROM t", "postgres", None),
        None
    );
}

#[test]
fn too_many_columns_are_not_typed() {
    let items: Vec<String> = (0..5000).map(|i| format!("{i} AS c{i}")).collect();
    let sql = format!("SELECT {}", items.join(", "));
    let (columns, found) = analyzer::analyze_with_diagnostics(&sql, "postgres", None);
    let names: Vec<String> = columns
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["*"]);
    let messages: Vec<String> = found.into_iter().map(|d| d.message).collect();
    assert_eq!(messages, ["over 4096 result columns are not typed"]);
    let columns = analyzer::analyze_select(&sql, "postgres", None).unwrap_or_default();
    assert_eq!(columns.len(), 1);
}

#[test]
fn completions_qualify_columns_of_several_relations() {
    let schema = analyzer::load_schema_with_skipped(
        "CREATE TABLE t (a INT NOT NULL); CREATE TABLE u (b TEXT); CREATE VIEW v AS SELECT a FROM t",
        "postgres",
    )
    .0;
    let labels = |query: &str| -> Vec<String> {
        let cursor = query.find('|').unwrap_or_default();
        let query = query.replacen('|', "", 1);
        analyzer::complete(&query, cursor, "postgres", Some(&schema), 10)
            .candidates
            .into_iter()
            .filter(|c| c.detail.is_some())
            .map(|c| c.label)
            .collect()
    };
    assert_eq!(labels("SELECT | FROM t, u"), ["t.a", "u.b"]);
    assert_eq!(labels("SELECT | FROM v, nope WHERE ("), ["a"]);
}
