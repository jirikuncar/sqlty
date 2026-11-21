//! The generated bench inputs are typed by the engine the benches time.

#![expect(
    clippy::expect_used,
    reason = "bench input is generated, never user data"
)]

use sqlty_bench::{
    one_query_cases, one_query_module, one_query_schema, python_source, schema_sql, QUERIES,
};
use sqlty_sql::analyzer;

#[test]
fn every_query_reads_the_schema() {
    let (schema, skipped) = analyzer::load_schema_with_skipped(&schema_sql(4), "postgres");
    assert!(skipped.is_empty(), "{skipped:?}");
    for (name, sql) in QUERIES {
        let (columns, _) = analyzer::analyze_with_diagnostics(sql, "postgres", Some(&schema));
        assert!(columns.is_some(), "{name}");
    }
}

#[test]
fn the_module_holds_one_distinct_query_per_handler() {
    let source = python_source(QUERIES.len() + 1);
    assert_eq!(source.matches("--sql").count(), QUERIES.len() + 1);
    assert!(source.contains(&format!("-- {}\")", QUERIES.len())));
}

#[test]
fn every_one_query_case_is_typed_without_diagnostics() {
    let (schema, skipped) = analyzer::load_schema_with_skipped(&one_query_schema(65), "postgres");
    assert!(skipped.is_empty(), "{skipped:?}");
    let cases = one_query_cases();
    assert_eq!(cases.len(), 16);
    assert_eq!(cases[3].0, "typical/aggregate");
    for (name, sql) in &cases {
        let (columns, diagnostics) =
            analyzer::analyze_with_diagnostics(sql, "postgres", Some(&schema));
        assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");
        let columns = columns.expect("the query is typed");
        assert!(
            columns.iter().all(|column| column.python_type.is_some()),
            "{name}"
        );
        assert!(one_query_module(sql).starts_with("q = \"--sql\\n"));
    }
}
