//! Engine benchmarks: `make bench-rust` (or `cargo bench --manifest-path rust/Cargo.toml`).
//! Complements `make bench-ci` (end-to-end CLI wall time and RSS against a budget).

#![expect(clippy::expect_used, reason = "a failed setup fails the benchmark")]

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use sqlty_bench::{
    one_query_cases, one_query_module, one_query_schema, python_source, schema_sql, QUERIES,
};
use sqlty_project::{check_module, Checker};
use sqlty_python::locate::Queries;
use sqlty_python::rewriter::ImportStyle;
use sqlty_python::types::symbols::Source;
use sqlty_python::{checker, options::Options, rewriter, rules::RuleConfig};
use sqlty_sql::analyzer;
use std::collections::HashSet;
use std::hint::black_box;
use std::sync::Arc;

fn catalog_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("catalog_load");
    group.sample_size(10);
    for tables in [50, 270, 540, 5000] {
        let sql = schema_sql(tables);
        group.throughput(Throughput::Bytes(sql.len() as u64));
        group.bench_function(format!("{tables}_tables"), |b| {
            b.iter(|| analyzer::load_schema_with_skipped(black_box(&sql), "postgres"));
        });
    }
    group.finish();
}

fn analyze(c: &mut Criterion) {
    let (schema, _) = analyzer::load_schema_with_skipped(&schema_sql(270), "postgres");
    let mut group = c.benchmark_group("analyze");
    for (name, sql) in QUERIES {
        group.bench_function(*name, |b| {
            b.iter(|| {
                analyzer::analyze_with_diagnostics(black_box(sql), "postgres", Some(&schema))
            });
        });
    }
    group.bench_function("all_no_schema", |b| {
        b.iter(|| {
            for (_, sql) in QUERIES {
                black_box(analyzer::analyze_select(sql, "postgres", None));
            }
        });
    });
    group.finish();
}

fn python_source_passes(c: &mut Criterion) {
    let (schema, _) = analyzer::load_schema_with_skipped(&schema_sql(270), "postgres");
    let rules = RuleConfig::default();
    let options = Options::new("postgres", Some(&schema), None, false).expect("valid options");
    let mut group = c.benchmark_group("python");
    group.sample_size(10);
    for queries in [1_000, 4_000] {
        let source = python_source(queries);
        group.throughput(Throughput::Elements(queries as u64));
        group.bench_function(format!("check_source_{queries}"), |b| {
            b.iter(|| checker::check_source(black_box(&source), &options, &rules, &[]));
        });
        group.bench_function(format!("rewrite_source_{queries}"), |b| {
            b.iter(|| rewriter::rewrite_source_with(black_box(&source), &options, false));
        });
    }
    group.finish();
}

fn name_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("name_index");
    for tables in [50, 540, 5000] {
        let (schema, _) = analyzer::load_schema_with_skipped(&schema_sql(tables), "postgres");
        group.bench_function(format!("relation_prefix_{tables}_tables"), |b| {
            b.iter(|| schema.relations_with_prefix(black_box("t004"), None, 20));
        });
        group.bench_function(format!("qualified_relation_prefix_{tables}_tables"), |b| {
            b.iter(|| schema.relations_with_prefix(black_box("v00"), Some("app"), 20));
        });
        group.bench_function(format!("function_prefix_{tables}_tables"), |b| {
            b.iter(|| schema.functions_with_prefix(black_box("f00"), None, 20));
        });
        let queries = [
            ("complete_relation", "SELECT * FROM t004", 18),
            (
                "complete_alias_columns",
                "SELECT a. FROM app.t0000 a JOIN app.t0001 b ON b.parent_id = a.id",
                9,
            ),
            (
                "complete_columns_in_scope",
                "SELECT * FROM app.t0000 a JOIN app.t0001 b ON b.parent_id = a.id WHERE s",
                72,
            ),
        ];
        for (name, query, cursor) in queries {
            group.bench_function(format!("{name}_{tables}_tables"), |b| {
                b.iter(|| {
                    analyzer::complete(black_box(query), cursor, "postgres", Some(&schema), 200)
                });
            });
        }
    }
    group.finish();
}

/// Locating the query under a cursor in a module of 4000 queries: one scan per buffer
/// version, then a binary search per request.
fn query_at(c: &mut Criterion) {
    let source = python_source(4_000);
    let style = ImportStyle::parse(None).expect("the default import style");
    let cursor = source.rfind("SELECT").expect("a query") + 7;
    let mut group = c.benchmark_group("query_at");
    group.bench_function("scan_4000_queries", |b| {
        b.iter(|| Queries::new(black_box(&source), &style, &[]));
    });
    let queries = Queries::new(&source, &style, &[]);
    assert!(queries.at(cursor).is_some(), "the cursor is in a query");
    group.bench_function("lookup_4000_queries", |b| {
        b.iter(|| queries.at(black_box(cursor)));
    });
    group.finish();
}

/// `sqlty check` of a module holding one query, as `sqlty rewrite` typed it, against a
/// 500-table schema loaded once: the parse of the module, the query's analysis and every
/// rule: what AGENTS.md "Check budget" bounds. `bench-ci` measures the typical queries
/// (`benchmarks/one_query.sql`) too, as `one_query_us`.
fn one_query(c: &mut Criterion) {
    let (schema, skipped) = analyzer::load_schema_with_skipped(&one_query_schema(500), "postgres");
    assert!(skipped.is_empty(), "{skipped:?}");
    let coerce = HashSet::new();
    let checker = Checker::new(
        "postgres",
        Some(&schema),
        None,
        false,
        RuleConfig::default(),
        None,
        &coerce,
        None,
    )
    .expect("valid settings");
    let check = |module: &str| {
        let parsed = Source::new(module, (3, 14)).map(Arc::new);
        check_module(module, parsed.as_ref(), None, &checker)
    };
    let mut group = c.benchmark_group("one_query");
    for (name, sql) in one_query_cases() {
        let module =
            rewriter::rewrite_source_with(&one_query_module(&sql), &checker.options, false);
        assert!(module.contains("SQL["), "{name} is typed: {module}");
        let found = check(&module);
        assert!(
            found.diagnostics.is_empty(),
            "{name}: {:?}",
            found.diagnostics
        );
        group.bench_function(name, |b| b.iter(|| check(black_box(&module))));
    }
    group.finish();
}

criterion_group!(
    benches,
    catalog_load,
    analyze,
    python_source_passes,
    name_index,
    query_at,
    one_query
);
criterion_main!(benches);
