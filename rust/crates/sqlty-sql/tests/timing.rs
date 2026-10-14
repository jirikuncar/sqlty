//! Reproducible analyzer timing. Run in release mode:
//! `cargo test --release -p sqlty-sql --test timing -- --ignored --nocapture`

use std::hint::black_box;
use std::time::Instant;

use sqlty_sql::analyzer::{analyze_with_diagnostics, load_schema_with_skipped};

const SCHEMA: &str = "
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL, email TEXT, age INT, meta JSONB);
CREATE TABLE orders (id INT PRIMARY KEY, user_id INT NOT NULL, total NUMERIC NOT NULL, status TEXT);
CREATE TABLE items (id INT PRIMARY KEY, order_id INT NOT NULL, sku TEXT NOT NULL, qty INT NOT NULL);
";

fn nested_scalar(depth: usize) -> String {
    let mut inner = "SELECT max(o.total) FROM orders o WHERE o.user_id = u.id".to_string();
    for level in 0..depth {
        inner = format!(
            "SELECT max(coalesce(({inner}), 0)) + (SELECT count(*) FROM items i WHERE i.order_id = u.id AND i.qty > {level}) FROM users x WHERE x.id = u.id"
        );
    }
    format!("SELECT u.id, ({inner}) AS v FROM users u")
}

fn queries() -> Vec<(&'static str, String)> {
    let and_chain = (0..30)
        .map(|i| format!("u.age > {i} AND u.email IS NOT NULL"))
        .collect::<Vec<_>>()
        .join(" AND ");
    vec![
        ("simple", "SELECT id, name, email FROM users WHERE id = $1".into()),
        ("join", "SELECT u.name, o.total, i.sku FROM users u JOIN orders o ON o.user_id = u.id JOIN items i ON i.order_id = o.id".into()),
        ("scalar-depth-3", nested_scalar(3)),
        ("scalar-depth-5", nested_scalar(5)),
        ("scalar-depth-8", nested_scalar(8)),
        ("and-chain-30", format!("SELECT CASE WHEN {and_chain} THEN u.email ELSE u.name END FROM users u WHERE {and_chain}")),
        ("cte-chain", "WITH a AS (SELECT id, email FROM users), b AS (SELECT a.id, a.email FROM a), c AS (SELECT b.id, b.email FROM b) SELECT c.id, c.email FROM c".into()),
        ("recursive", "WITH RECURSIVE t(n, m) AS (SELECT 1, 'a'::text UNION ALL SELECT n + 1, m FROM t WHERE n < 5) SELECT n, m FROM t".into()),
        ("json", "SELECT json_build_object('id', u.id, 'orders', (SELECT json_agg(json_build_object('t', o.total, 'n', (SELECT count(*) FROM items i WHERE i.order_id = o.id))) FROM orders o WHERE o.user_id = u.id)) FROM users u".into()),
    ]
}

#[test]
#[ignore = "timing; run with --release --ignored --nocapture"]
fn analyzer_timing() {
    let (schema, _) = load_schema_with_skipped(SCHEMA, "postgres");
    let only = std::env::var("SQLTY_TIMING_ONLY").ok();
    for (name, sql) in queries() {
        if only.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        let iterations = std::env::var("SQLTY_TIMING_ITERS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(50);
        let first = analyze_with_diagnostics(&sql, "postgres", Some(&schema));
        let shape = first.0.as_ref().map(|c| c.as_ref().map(Vec::len));
        println!("{name}: columns {shape:?}, diagnostics {}", first.1.len());
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = black_box(analyze_with_diagnostics(
                black_box(&sql),
                "postgres",
                Some(&schema),
            ));
        }
        let per = start.elapsed() / iterations;
        println!("{name:16} {per:>12.2?}");
    }
}
