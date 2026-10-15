//! Engine benchmarks: `make bench-rust` (or `cargo bench --manifest-path rust/Cargo.toml`).
//! Complements `make bench-ci` (end-to-end CLI wall time and RSS against a budget).

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use sqlty_bench::{python_source, schema_sql, QUERIES};
use sqlty_python::{checker, options::Options, rewriter, rules::RuleConfig};
use sqlty_sql::analyzer;
use std::hint::black_box;

fn catalog_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("catalog_load");
    group.sample_size(10);
    for tables in [50, 270] {
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
                black_box(analyzer::analyze_select(sql, "postgres", None).ok());
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

criterion_group!(benches, catalog_load, analyze, python_source_passes);
criterion_main!(benches);
