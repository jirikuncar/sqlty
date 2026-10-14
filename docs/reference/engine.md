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

## Placeholders

A placeholder is a value of unknown type, so it does not stop inference.
What each dialect reads:

| Placeholder | PostgreSQL | MySQL | SQLite |
|---|---|---|---|
| `%s`, `%b`, `%t`, `%(name)s` (psycopg, PyMySQL, mysqlclient) | yes | yes | yes |
| `%%` (an escaped `%`) | yes | yes | yes |
| `$1` | yes | yes | yes |
| `?` | yes, unless the query also uses a JSONB `?` operator | yes | yes |
| `:name` | yes | yes | yes |

The `%` forms are replaced before parsing, everywhere in the query, as
psycopg does: write `'a%%'` for a literal `%` in a string.

!!! warning "MySQL servers reject the `--sql` marker"
    The marker line is part of the string the driver sends, and MySQL and
    MariaDB read `--` as a comment only before whitespace. Use `-- sql`
    instead; see [`mysql-sql-marker`](rules.md#mysql-sql-marker).

## Known gaps

- **SQLite free-form type names.** SQLite accepts any type name and derives an
  affinity (sqlite.org/datatype3.html section 3.1), but `UNSIGNED BIG INT`,
  `VARYING CHARACTER(255)` or `NATIVE CHARACTER(70)` make sqlty skip the
  table.

Earlier versions also shipped a pure-Python backend built on
[sqlglot](https://sqlglot.com). Removing it changed:

- Unaliased calls to functions sqlty can't type are named after the function
  (`NULLIF(a, b)` is `nullif`, not an empty name) in `column-name-mismatch`
  and `typeddict-key-mismatch`.
- The magic codec adds imports after the module header, as `sqlty rewrite`
  does, instead of placing them with isort, and turns `# coding: sqlty` into
  `# coding: utf-8` in the decoded text.
- Dialects other than PostgreSQL, MySQL and SQLite follow sqlparser-rs rather
  than sqlglot.
- An unsupported `import-style` is an error instead of falling back to
  `from sqlty import SQL`.
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
| `rewrite_source`, 3 queries | 122 µs |
| `rewrite_source`, 20 queries | 343 µs |
| `strip_source`, 1 query | 135 µs |
| `strip_source`, 20 queries | 134 µs |
| `check_source`, 4 queries | 754 µs |

Whole-file operations carry a fixed cost per call for compiling their
patterns. Numbers vary between machines and runs. A debug build from plain
`maturin develop` is much slower, so build in release mode first:

```bash
uv run maturin develop --release
uv run python benchmarks/bench_core.py
```
