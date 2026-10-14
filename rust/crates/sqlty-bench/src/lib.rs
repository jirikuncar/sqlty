//! Synthetic inputs shared by the benches; deterministic, no randomness.
//!
//! The Criterion benchmarks live in `benches/engine.rs` (run them with `make bench-rust`).
//! They feed the engine generated input of a chosen size:
//!
//! - [`schema_sql`]: a PostgreSQL schema of N tables with views, functions, an enum and a
//!   domain, to time schema loading.
//! - [`python_source`]: Python source with N `--sql` queries, to time rewriting and checking.
//!
//! ```
//! let schema = sqlty_bench::schema_sql(2);
//! assert!(schema.contains("CREATE TABLE app.t0001"));
//!
//! let source = sqlty_bench::python_source(3);
//! assert_eq!(source.matches("--sql").count(), 3);
//! ```

use std::fmt::Write;

/// PostgreSQL schema of `tables` tables, each with a view, a scalar function and a
/// `SETOF` function (the shape of the #227 slow-load report), plus an enum and a domain.
///
/// # Panics
///
/// Never: writing to a `String` cannot fail.
#[must_use]
pub fn schema_sql(tables: usize) -> String {
    let mut out = String::from(
        "CREATE SCHEMA app;\n\
         CREATE TYPE app.status AS ENUM ('new', 'active');\n\
         ALTER TYPE app.status ADD VALUE 'closed';\n\
         CREATE DOMAIN app.percent AS integer CHECK (VALUE BETWEEN 0 AND 100);\n",
    );
    for i in 0..tables {
        let parent = i.saturating_sub(1);
        writeln!(
            out,
            "CREATE TABLE app.t{i:04} (id integer PRIMARY KEY, \
             parent_id integer REFERENCES app.t{parent:04}(id), name text NOT NULL, \
             status app.status NOT NULL DEFAULT 'new', score integer CHECK (score >= 0), \
             created_at timestamptz NOT NULL DEFAULT now(), tags text[], payload jsonb, \
             CONSTRAINT t{i:04}_name_len CHECK (length(name) < 200));"
        )
        .unwrap();
        writeln!(
            out,
            "ALTER TABLE app.t{i:04} ALTER COLUMN score SET NOT NULL;"
        )
        .unwrap();
        if i % 3 == 0 {
            writeln!(out, "ALTER TABLE app.t{i:04} ADD COLUMN note text;").unwrap();
        }
        writeln!(
            out,
            "CREATE VIEW app.v{i:04} AS SELECT id, name FROM app.t{i:04} WHERE score >= 0;"
        )
        .unwrap();
        writeln!(
            out,
            "CREATE FUNCTION app.f{i:04}(integer) RETURNS bigint LANGUAGE sql \
             AS 'SELECT count(*) FROM app.t{i:04}';"
        )
        .unwrap();
        writeln!(
            out,
            "CREATE FUNCTION app.s{i:04}(integer) RETURNS SETOF app.t{i:04} LANGUAGE sql \
             AS 'SELECT * FROM app.t{i:04} WHERE id = $1';"
        )
        .unwrap();
    }
    out.push_str("SET search_path = app, public;\n");
    out
}

/// Representative queries against [`schema_sql`] tables `t0000` and `t0001`.
pub const QUERIES: &[(&str, &str)] = &[
    ("simple", "SELECT id::INTEGER, name FROM app.t0000"),
    ("star", "SELECT * FROM app.t0000 WHERE id = $1"),
    (
        "join",
        "SELECT a.id, b.name, a.created_at FROM app.t0000 a JOIN app.t0001 b ON b.parent_id = a.id WHERE a.score > $1",
    ),
    (
        "aggregate",
        "SELECT status, count(*) AS n, max(created_at) AS last FROM app.t0000 GROUP BY status HAVING count(*) > 1",
    ),
    (
        "cte_subquery",
        "WITH recent AS (SELECT id, name FROM app.t0000 WHERE created_at > now() - interval '1 day') \
         SELECT r.id, (SELECT count(*) FROM app.t0001 WHERE parent_id = r.id) AS kids FROM recent r",
    ),
    ("view", "SELECT id, name FROM app.v0000"),
    ("function", "SELECT app.f0000(id) AS c FROM app.t0000"),
    ("setof", "SELECT id, name FROM app.s0000($1)"),
    (
        "json",
        "SELECT payload->'a' AS a, payload->>'b' AS b, tags[1] AS first FROM app.t0000",
    ),
    (
        "insert_returning",
        "INSERT INTO app.t0000 (name, score) VALUES ($1, $2) RETURNING id, created_at",
    ),
];

/// A Python module of `queries` `--sql` query strings, cycling through [`QUERIES`].
///
/// # Panics
///
/// Never: writing to a `String` cannot fail.
#[must_use]
pub fn python_source(queries: usize) -> String {
    let mut out = String::from("from typing import TypedDict\n\n\n");
    for i in 0..queries {
        let (_, sql) = QUERIES[i % QUERIES.len()];
        writeln!(
            out,
            "def handler_{i}(conn):\n    return conn.execute(\"--sql\\n{}\")\n",
            sql.replace('"', "'")
        )
        .unwrap();
    }
    out
}
