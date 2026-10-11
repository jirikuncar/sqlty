"""Benchmarks of the ``sqlty.core`` functions.

Build the extension in release mode first: ``uv run maturin develop --release``.

Usage:
    uv run python benchmarks/bench_core.py
    uv run pytest benchmarks/bench_core.py -v  # also works as pytest
"""

from __future__ import annotations

import statistics
import time

# ---------------------------------------------------------------------------
# Test fixtures — increasingly complex queries
# ---------------------------------------------------------------------------

SIMPLE_QUERY = "SELECT 1::INTEGER"

MEDIUM_QUERY = "SELECT id::INTEGER, name::TEXT, email::TEXT, active::BOOLEAN FROM users"

COMPLEX_QUERY = """\
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    u.email::TEXT,
    u.active::BOOLEAN,
    u.created_at::TIMESTAMP,
    u.updated_at::TIMESTAMP,
    o.total::FLOAT,
    o.status::TEXT,
    o.shipped_at::TIMESTAMP,
    p.title::TEXT
FROM users u
JOIN orders o ON o.user_id = u.id
LEFT JOIN products p ON p.id = o.product_id
WHERE u.active = true
ORDER BY o.created_at DESC
LIMIT 100
"""

SCHEMA_SQL = """\
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE,
    active BOOLEAN DEFAULT true,
    role TEXT DEFAULT 'user',
    created_at TIMESTAMP DEFAULT NOW()
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    total FLOAT NOT NULL,
    status TEXT DEFAULT 'pending',
    shipped_at TIMESTAMP
);
"""

STAR_QUERY = "SELECT * FROM users"

REWRITE_SOURCE_SMALL = """\
q = "--sql\\nSELECT id::INTEGER, name::TEXT FROM users"
"""

REWRITE_SOURCE_MEDIUM = """\
q1 = "--sql\\nSELECT id::INTEGER FROM users"
q2 = "--sql\\nSELECT name::TEXT, email::TEXT FROM users"
q3 = "--sql\\nSELECT created_at::TIMESTAMP FROM users"
"""

REWRITE_SOURCE_LARGE = "\n".join(
    f'q{i} = "--sql\\nSELECT id::INTEGER, name::TEXT, val::FLOAT FROM t{i}"' for i in range(20)
)

# Pre-rewritten source for strip benchmarks
STRIP_SOURCE_SMALL = """\
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
"""

STRIP_SOURCE_LARGE = "from sqlty import SQL\n" + "\n".join(
    f'q{i} = SQL[tuple[int, str, float]]("--sql\\nSELECT id::INTEGER, name::TEXT, val::FLOAT FROM t{i}")'
    for i in range(20)
)

# Pre-rewritten source for check benchmarks
CHECK_SOURCE = """\
from sqlty import SQL
q1 = SQL[tuple[int, str]]("--sql\\nSELECT id::INTEGER, name::TEXT FROM users")
q2 = SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM orders")
q3 = "--sql\\nSELECT name::TEXT FROM users"
"""


# ---------------------------------------------------------------------------
# Benchmark harness
# ---------------------------------------------------------------------------


def bench(fn, *, warmup: int = 50, iterations: int = 500, label: str = "") -> dict:
    """Run fn() `iterations` times (after warmup) and return timing stats."""
    for _ in range(warmup):
        fn()

    times: list[float] = []
    for _ in range(iterations):
        t0 = time.perf_counter_ns()
        fn()
        t1 = time.perf_counter_ns()
        times.append((t1 - t0) / 1_000)  # nanoseconds -> microseconds

    return {
        "label": label,
        "median_us": statistics.median(times),
        "mean_us": statistics.mean(times),
        "p95_us": sorted(times)[int(0.95 * len(times))],
        "min_us": min(times),
        "iterations": iterations,
    }


def run_benchmarks() -> list[dict]:
    from sqlty import core

    cases = {
        "infer_type: simple (1 col)": lambda: core.infer_type(SIMPLE_QUERY),
        "infer_type: medium (4 cols)": lambda: core.infer_type(MEDIUM_QUERY),
        "infer_type: complex (10 cols, JOINs)": lambda: core.infer_type(COMPLEX_QUERY),
        "infer_type: schema-aware": lambda: core.infer_type(
            "SELECT id, name FROM users", schema_sql=SCHEMA_SQL
        ),
        "expand_star: with schema": lambda: core.expand_star(STAR_QUERY, schema_sql=SCHEMA_SQL),
        "rewrite_source: 1 query": lambda: core.rewrite_source(REWRITE_SOURCE_SMALL),
        "rewrite_source: 3 queries": lambda: core.rewrite_source(REWRITE_SOURCE_MEDIUM),
        "rewrite_source: 20 queries": lambda: core.rewrite_source(REWRITE_SOURCE_LARGE),
        "strip_source: 1 query": lambda: core.strip_source(STRIP_SOURCE_SMALL),
        "strip_source: 20 queries": lambda: core.strip_source(STRIP_SOURCE_LARGE),
        "check_source: 4 queries": lambda: core.check_source(CHECK_SOURCE),
    }
    results = [bench(fn, label=label) for label, fn in cases.items()]
    print(f"  {'Benchmark':<42s}  {'Median':>10s}  {'p95':>10s}")
    print(f"  {'-' * 42}  {'-' * 10}  {'-' * 10}")
    for r in results:
        print(f"  {r['label']:<42s}  {r['median_us']:>7.0f} us  {r['p95_us']:>7.0f} us")
    print()
    return results


def test_benchmark_calls():
    """Smoke test: every benchmarked call does its work."""
    from sqlty import core

    assert core.infer_type(SIMPLE_QUERY) is not None
    assert core.rewrite_source(REWRITE_SOURCE_LARGE) != REWRITE_SOURCE_LARGE
    assert core.strip_source(STRIP_SOURCE_SMALL) != STRIP_SOURCE_SMALL
    assert core.check_source(CHECK_SOURCE)


if __name__ == "__main__":
    run_benchmarks()
