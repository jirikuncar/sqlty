# Backends

sqlty's core operations (type inference, `SELECT *` expansion, rewrite,
strip and check) have two implementations:

- **Rust**: the `sqlty._core` extension, built with
  [PyO3](https://pyo3.rs) and [maturin](https://www.maturin.rs), parsing SQL
  with [sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs).
- **Pure Python**: `sqlty._core_py`, parsing SQL with
  [sqlglot](https://sqlglot.com).

## Choosing a backend

`sqlty.core` uses the Rust extension when it can be imported and falls back to
Python otherwise. Wheels built by maturin include the extension; building
from source needs a Rust toolchain. Set `SQLTY_LIB` to force a backend:

```bash
SQLTY_LIB=py sqlty check .   # pure Python
SQLTY_LIB=rs sqlty check .   # Rust; fails if the extension is missing
```

The CLI (`rewrite`, `strip`, `check`) and the
[`sqlty.core`](python-api.md#sqltycore) functions go through this switch.
Row type validation in `check` (TypedDict, dataclass, ...) runs in Python on
top of either backend. The [magic codec](../guide/magic-codec.md) and
[stub generation](../guide/stubs.md) always use the Python implementation,
as do the tested examples in this documentation.

## Parity

Both backends must give the same results. The guarantees are tested, not
assumed:

- Every [mdtest](https://github.com/jirikuncar/sqlty/tree/main/tests/mdtest)
  runs against every available backend: inferred types, rewrite and strip
  output, and check diagnostics.
  [`tests/test_core_invariants.py`](https://github.com/jirikuncar/sqlty/blob/main/tests/test_core_invariants.py)
  adds idempotency (`rewrite(rewrite(x)) == rewrite(x)`), round trips, and
  that string contents are never modified.
- CI runs the test suite with the Rust extension, and again without it
  (except the end-to-end tests).
- Tests compare the SQL type tables of both backends to catch drift.

## Known gaps

Known differences, and gaps both backends share, are tracked in
[PLAN-FUTURE.md](https://github.com/jirikuncar/sqlty/blob/main/PLAN-FUTURE.md#backend-parity-gaps).
In short:

- In PostgreSQL, `?` is a placeholder only in Python (see
  [Placeholders](#placeholders)).
- MySQL `INT3 UNSIGNED` / `MIDDLEINT UNSIGNED` columns parse in Python but
  make Rust skip the table.
- MySQL `ZEROFILL` columns make both backends skip the table.
- SQLite free-form type names (`UNSIGNED BIG INT`, `VARYING CHARACTER(255)`)
  make both backends skip the table.
- Rust reads dialects other than PostgreSQL, MySQL and SQLite as generic SQL;
  only those three are kept in parity.

## Placeholders

A placeholder is a value of unknown type, so it does not stop inference.
Which placeholders each backend reads, per dialect:

| Placeholder | PostgreSQL | MySQL | SQLite |
|---|---|---|---|
| `%s`, `%b`, `%t`, `%(name)s` (psycopg, PyMySQL, mysqlclient) | both | both | both |
| `%%` (an escaped `%`) | both | both | both |
| `$1` | both | both | both |
| `?` | Python only | both | both |
| `:name` | both | both | both |

The `%` forms are replaced before parsing, everywhere in the query, as
psycopg does: write `'a%%'` for a literal `%` in a string.

!!! warning "MySQL servers reject the `--sql` marker"
    sqlty infers MySQL queries, `%s` included, but the `--sql` marker line is
    part of the string the driver sends. MySQL and MariaDB read `--` as a
    comment only when whitespace follows it, so a query that starts with
    `--sql\n` fails at runtime with a syntax error (1064). See
    [Schema inference](../guide/schema-inference.md#mysql).

## Benchmarks

Median time per call from `benchmarks/bench_core.py` (500 iterations after
warm-up), on a 4-core Linux x86_64 machine:

| Operation | Python | Rust | Speedup |
|-----------|-------:|-----:|--------:|
| `infer_type`, 1 column | 177 µs | 5 µs | 32x |
| `infer_type`, 4 columns | 613 µs | 13 µs | 45x |
| `infer_type`, 10 columns with JOINs | 2152 µs | 59 µs | 36x |
| `infer_type` with a schema | 3154 µs | 54 µs | 59x |
| `expand_star` with a schema | 2896 µs | 83 µs | 35x |
| `rewrite_source`, 1 query | 72 µs | 135 µs | 0.5x |
| `rewrite_source`, 20 queries | 849 µs | 144 µs | 5.9x |
| `strip_source`, 20 queries | 81 µs | 278 µs | 0.3x |
| `check_source`, 4 queries | 804 µs | 551 µs | 1.5x |

Parsing SQL is where Rust wins. Whole-file operations carry a fixed cost per
call in the Rust backend, so for small files they are on par with Python or
slower; `strip_source` is regex-only and faster in Python. Numbers vary
between machines and runs.

To reproduce them from a checkout, build the extension in release mode
first. A debug build from plain `maturin develop` is much slower than
Python:

```bash
uv run maturin develop --release
uv run python benchmarks/bench_core.py
```
