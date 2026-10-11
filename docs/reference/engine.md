# Engine

sqlty's core operations (type inference, `SELECT *` expansion, rewrite,
strip, check and fix) run in one engine written in Rust: the `sqlty._core`
extension, built with [PyO3](https://pyo3.rs) and
[maturin](https://www.maturin.rs), parsing SQL with
[sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs). The CLI,
the [`sqlty.core`](python-api.md#sqltycore) functions, the
[magic codec](../guide/magic-codec.md) and [stub generation](../guide/stubs.md)
all use it.

Python handles what needs the Python runtime: resolving row types such as
TypedDicts and dataclasses for `check`, configuration, the CLI and the
psycopg helpers.

Wheels include the compiled extension; building from source needs a
[Rust toolchain](https://rustup.rs/). There is no pure-Python fallback.

## Dialects

`dialect` picks the sqlparser-rs dialect that reads queries and schemas:
`postgres` (default), `mysql`, `sqlite`, and any other name sqlparser-rs
knows (`bigquery`, `snowflake`, `duckdb`, `clickhouse`, `mssql` or `tsql`,
`redshift`, `oracle`, ...). Unknown names read as generic SQL. Type names are
spelled the PostgreSQL way, so `INT64` expands to `BIGINT` and `NUMBER` to
`DECIMAL`. PostgreSQL, MySQL and SQLite are specified by the test suite; the
others work on a best-effort basis. The `--dialect` CLI flag accepts the
first three; set others with `dialect` in `pyproject.toml`.

Query parameters are values of unknown type: psycopg's `%s` and `%(name)s`
in every dialect, and `$1`, `:name` or `?` where the dialect reads them. In
PostgreSQL, `?` is a placeholder only where it isn't the JSONB operator.

## Known gaps

- **SQLite free-form type names.** SQLite accepts any type name and derives an
  affinity (sqlite.org/datatype3.html section 3.1), but `UNSIGNED BIG INT`,
  `VARYING CHARACTER(255)` or `NATIVE CHARACTER(70)` make sqlty skip the
  table.

Earlier versions also shipped a pure-Python backend built on
[sqlglot](https://sqlglot.com). Removing it changed:

- Unaliased calls to functions sqlty can't type are named after the function
  (`NULLIF(a, b)` is `NULLIF`, not an empty name) in `column-name-mismatch`
  and `typeddict-key-mismatch`.
- The magic codec adds imports after the module header, as `sqlty rewrite`
  does, instead of placing them with isort, and turns `# coding: sqlty` into
  `# coding: utf-8` in the decoded text.
- Dialects other than PostgreSQL, MySQL and SQLite follow sqlparser-rs rather
  than sqlglot.
- `SQLTY_LIB` is gone.

## Benchmarks

Median time per call from `benchmarks/bench_core.py` (500 iterations after
warm-up, release build), on a 4-core Linux x86_64 machine:

| Operation | Time |
|-----------|-----:|
| `infer_type`, 1 column | 5 µs |
| `infer_type`, 4 columns | 12 µs |
| `infer_type`, 10 columns with JOINs | 50 µs |
| `infer_type` with a schema | 43 µs |
| `expand_star` with a schema | 42 µs |
| `rewrite_source`, 1 query | 105 µs |
| `rewrite_source`, 20 queries | 343 µs |
| `strip_source`, 20 queries | 134 µs |
| `check_source`, 4 queries | 754 µs |

Whole-file operations carry a fixed cost per call for compiling their
patterns. Numbers vary between machines and runs. A debug build from plain
`maturin develop` is much slower, so build in release mode first:

```bash
uv run maturin develop --release
uv run python benchmarks/bench_core.py
```
