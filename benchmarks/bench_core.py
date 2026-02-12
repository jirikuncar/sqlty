"""Benchmarks comparing Python and Rust backends.

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
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
"""

REWRITE_SOURCE_MEDIUM = """\
q1 = "--sql SELECT id::INTEGER FROM users"
q2 = "--sql SELECT name::TEXT, email::TEXT FROM users"
q3 = "--sql SELECT created_at::TIMESTAMP FROM users"
"""

REWRITE_SOURCE_LARGE = "\n".join(
    f'q{i} = "--sql SELECT id::INTEGER, name::TEXT, val::FLOAT FROM t{i}"' for i in range(20)
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
q3 = "--sql SELECT name::TEXT FROM users"
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


def format_result(py: dict, rs: dict | None) -> str:
    """Format a benchmark comparison row."""
    label = py["label"]
    py_us = py["median_us"]
    if rs is None:
        return f"  {label:<42s}  {py_us:>10.0f} us  {'N/A':>10s}     {'N/A':>5s}"
    rs_us = rs["median_us"]
    speedup = py_us / rs_us if rs_us > 0 else float("inf")
    return f"  {label:<42s}  {py_us:>10.0f} us  {rs_us:>10.0f} us  {speedup:>5.1f}x"


# ---------------------------------------------------------------------------
# Import both backends
# ---------------------------------------------------------------------------


def load_backends():
    """Import both Python and Rust backends."""
    import sqlty._core_py as py_mod

    try:
        import sqlty._core as rs_mod
    except ImportError:
        rs_mod = None

    return py_mod, rs_mod


# ---------------------------------------------------------------------------
# Benchmark definitions
# ---------------------------------------------------------------------------


def run_benchmarks():
    py, rs = load_backends()

    if rs is None:
        print("WARNING: Rust backend not available — showing Python-only timings.\n")
    else:
        print("Both backends loaded.\n")

    results: list[tuple[dict, dict | None]] = []

    def add(label: str, py_fn, rs_fn=None):
        py_result = bench(py_fn, label=label)
        rs_result = bench(rs_fn, label=label) if rs_fn else None
        results.append((py_result, rs_result))

    # --- infer_type ---
    add(
        "infer_type: simple (1 col)",
        lambda: py.infer_type(SIMPLE_QUERY),
        (lambda: rs.infer_type(SIMPLE_QUERY)) if rs else None,
    )

    add(
        "infer_type: medium (4 cols)",
        lambda: py.infer_type(MEDIUM_QUERY),
        (lambda: rs.infer_type(MEDIUM_QUERY)) if rs else None,
    )

    add(
        "infer_type: complex (10 cols, JOINs)",
        lambda: py.infer_type(COMPLEX_QUERY),
        (lambda: rs.infer_type(COMPLEX_QUERY)) if rs else None,
    )

    add(
        "infer_type: schema-aware",
        lambda: py.infer_type("SELECT id, name FROM users", schema_sql=SCHEMA_SQL),
        (lambda: rs.infer_type("SELECT id, name FROM users", schema_sql=SCHEMA_SQL))
        if rs
        else None,
    )

    # --- expand_star ---
    add(
        "expand_star: with schema",
        lambda: py.expand_star(STAR_QUERY, schema_sql=SCHEMA_SQL),
        (lambda: rs.expand_star(STAR_QUERY, schema_sql=SCHEMA_SQL)) if rs else None,
    )

    # --- rewrite_source ---
    add(
        "rewrite_source: 1 query",
        lambda: py.rewrite_source(REWRITE_SOURCE_SMALL),
        (lambda: rs.rewrite_source(REWRITE_SOURCE_SMALL)) if rs else None,
    )

    add(
        "rewrite_source: 3 queries",
        lambda: py.rewrite_source(REWRITE_SOURCE_MEDIUM),
        (lambda: rs.rewrite_source(REWRITE_SOURCE_MEDIUM)) if rs else None,
    )

    add(
        "rewrite_source: 20 queries",
        lambda: py.rewrite_source(REWRITE_SOURCE_LARGE),
        (lambda: rs.rewrite_source(REWRITE_SOURCE_LARGE)) if rs else None,
    )

    # --- strip_source ---
    add(
        "strip_source: 1 query",
        lambda: py.strip_source(STRIP_SOURCE_SMALL),
        (lambda: rs.strip_source(STRIP_SOURCE_SMALL)) if rs else None,
    )

    add(
        "strip_source: 20 queries",
        lambda: py.strip_source(STRIP_SOURCE_LARGE),
        (lambda: rs.strip_source(STRIP_SOURCE_LARGE)) if rs else None,
    )

    # --- check_source ---
    add(
        "check_source: 4 queries",
        lambda: py.check_source(CHECK_SOURCE),
        (lambda: rs.check_source(CHECK_SOURCE)) if rs else None,
    )

    # --- Print results ---
    print(f"  {'Benchmark':<42s}  {'Python':>10s}     {'Rust':>10s}  {'Speedup':>7s}")
    print(f"  {'-' * 42}  {'-' * 10}     {'-' * 10}  {'-' * 7}")
    for py_result, rs_result in results:
        print(format_result(py_result, rs_result))

    if rs:
        speedups = [
            py_r["median_us"] / rs_r["median_us"]
            for py_r, rs_r in results
            if rs_r is not None and rs_r["median_us"] > 0
        ]
        geo_mean = statistics.geometric_mean(speedups)
        print(f"\n  Geometric mean speedup: {geo_mean:.1f}x")

    print()
    return results


# ---------------------------------------------------------------------------
# pytest integration — lets `pytest benchmarks/` discover and run this
# ---------------------------------------------------------------------------


def test_benchmark_python_backend():
    """Smoke test: all Python backend calls succeed."""
    py, _ = load_backends()
    assert py.infer_type(SIMPLE_QUERY) is not None
    assert py.rewrite_source(REWRITE_SOURCE_SMALL) != REWRITE_SOURCE_SMALL
    assert py.strip_source(STRIP_SOURCE_SMALL) != STRIP_SOURCE_SMALL
    assert py.check_source(CHECK_SOURCE) is not None


def test_benchmark_rust_backend():
    """Smoke test: all Rust backend calls succeed (skip if not compiled)."""
    _, rs = load_backends()
    if rs is None:
        import pytest

        pytest.skip("Rust backend not compiled")
    assert rs.infer_type(SIMPLE_QUERY) is not None
    assert rs.rewrite_source(REWRITE_SOURCE_SMALL) != REWRITE_SOURCE_SMALL
    assert rs.strip_source(STRIP_SOURCE_SMALL) != STRIP_SOURCE_SMALL
    assert rs.check_source(CHECK_SOURCE) is not None


if __name__ == "__main__":
    run_benchmarks()
