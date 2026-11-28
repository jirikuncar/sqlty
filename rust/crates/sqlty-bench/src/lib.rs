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

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]
#![expect(
    clippy::unwrap_used,
    reason = "bench input is generated, never user data"
)]

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

/// A Python module of `queries` `--sql` query strings, cycling through [`QUERIES`]. Each
/// ends in a distinct comment, so no two strings are the same query.
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
            "def handler_{i}(conn):\n    return conn.execute(\"--sql\\n{} -- {i}\")\n",
            sql.replace('"', "'")
        )
        .unwrap();
    }
    out
}

/// Columns of `app.wide`, the table of [`one_query_schema`] that the column and
/// `GROUP BY` sweeps read.
pub const WIDE_COLUMNS: usize = 256;

/// [`schema_sql`] of `tables` tables plus `app.wide`, of [`WIDE_COLUMNS`] columns.
///
/// # Panics
///
/// Never: writing to a `String` cannot fail.
#[must_use]
pub fn one_query_schema(tables: usize) -> String {
    let mut out = schema_sql(tables);
    let columns: Vec<String> = (0..WIDE_COLUMNS)
        .map(|i| match i % 2 {
            0 => format!("c{i:03} integer NOT NULL"),
            _ => format!("c{i:03} text"),
        })
        .collect();
    writeln!(out, "CREATE TABLE app.wide ({});", columns.join(", ")).unwrap();
    out
}

/// `benchmarks/one_query.sql`: the typical queries `bench-ci` budgets too.
const TYPICAL: &str = include_str!("../../../../benchmarks/one_query.sql");

/// The queries of [`TYPICAL`] by name, each on one line.
pub fn typical_queries() -> impl Iterator<Item = (&'static str, String)> {
    TYPICAL
        .split("-- name: ")
        .skip(1)
        .filter_map(|block| block.split_once('\n'))
        .map(|(name, sql)| {
            (
                name.trim(),
                sql.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
}

/// One query each, by `group/name`, against [`one_query_schema`] of at least 65 tables:
/// typical queries, then sweeps of one dimension each (joins, selected columns, `IN` list
/// items, `GROUP BY` keys).
#[must_use]
pub fn one_query_cases() -> Vec<(String, String)> {
    let wide = |n: usize| (0..n).map(|i| format!("c{i:03}")).collect::<Vec<_>>();
    let mut cases: Vec<(String, String)> = typical_queries()
        .map(|(name, sql)| (format!("typical/{name}"), sql))
        .collect();
    for joins in [1, 4, 16, 64] {
        let chain: String = (1..=joins)
            .map(|i| format!(" JOIN app.t{i:04} j{i} ON j{i}.parent_id = j{}.id", i - 1))
            .collect();
        let sql = format!("SELECT j0.id, j{joins}.name FROM app.t0000 j0{chain} WHERE j0.id = $1");
        cases.push((format!("joins/{joins}"), sql));
    }
    for subqueries in [4, 64] {
        let chain: String = (1..=subqueries)
            .map(|i| format!(" JOIN app.t{i:04} j{i} ON j{i}.parent_id = j{}.id", i - 1))
            .collect();
        let exists: String = (1..=subqueries)
            .map(|i| {
                format!(
                    " AND EXISTS (SELECT 1 FROM app.t{i:04} b WHERE b.parent_id = j{}.id)",
                    i - 1
                )
            })
            .collect();
        let sql = format!("SELECT j0.id FROM app.t0000 j0{chain} WHERE j0.id = $1{exists}");
        cases.push((format!("correlated/{subqueries}"), sql));
    }
    for columns in [4, 32, WIDE_COLUMNS] {
        let sql = format!(
            "SELECT {} FROM app.wide WHERE c000 = $1",
            wide(columns).join(", ")
        );
        cases.push((format!("columns/{columns}"), sql));
    }
    for items in [10, 1_000] {
        let list: Vec<String> = (0..items).map(|i| i.to_string()).collect();
        let sql = format!(
            "SELECT id, name FROM app.t0042 WHERE id IN ({})",
            list.join(", ")
        );
        cases.push((format!("in_list/{items}"), sql));
    }
    for keys in [4, 32, WIDE_COLUMNS] {
        let list = wide(keys).join(", ");
        let sql = format!("SELECT {list}, count(*) AS n FROM app.wide GROUP BY {list}");
        cases.push((format!("group_by/{keys}"), sql));
    }
    cases
}

/// A Python module holding `sql` as its one `--sql` string, as `sqlty rewrite` reads it.
#[must_use]
pub fn one_query_module(sql: &str) -> String {
    format!("q = \"--sql\\n{sql}\"\n")
}
