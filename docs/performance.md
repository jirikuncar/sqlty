# Performance

## Two backends

sqlty's core operations (type inference, `SELECT *` expansion, rewrite,
strip and check) have two implementations:

- **Rust**: the `sqlty._core` extension, built with
  [PyO3](https://pyo3.rs) and [maturin](https://www.maturin.rs), parsing SQL
  with [sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs).
- **Pure Python**: `sqlty._core_py`, parsing SQL with
  [sqlglot](https://sqlglot.com).

`sqlty.core` uses the Rust extension when it can be imported and falls back to
Python otherwise. Wheels built by maturin include the extension; building
from source needs a Rust toolchain. Set `SQLTY_LIB` to force a backend:

```bash
SQLTY_LIB=py sqlty check .   # pure Python
SQLTY_LIB=rs sqlty check .   # Rust; fails if the extension is missing
```

The CLI (`rewrite`, `strip`, `check`) and the `sqlty.core` functions go
through this switch. Row type validation in `check` (TypedDict, dataclass,
...) runs in Python on top of either backend. The
[magic codec](guide/magic-codec.md) and [stub generation](guide/stubs.md)
always use the Python implementation, as do the tested examples in this
documentation.

## Benchmarks

Median time per call from `benchmarks/bench_core.py` (500 iterations after
warm-up), on a 4-core Linux x86_64 machine:

| Operation | Python | Rust | Speedup |
|-----------|-------:|-----:|--------:|
| `infer_type`, 1 column | 177 us | 5 us | 32x |
| `infer_type`, 4 columns | 613 us | 13 us | 45x |
| `infer_type`, 10 columns with JOINs | 2152 us | 59 us | 36x |
| `infer_type` with a schema | 3154 us | 54 us | 59x |
| `expand_star` with a schema | 2896 us | 83 us | 35x |
| `rewrite_source`, 1 query | 72 us | 135 us | 0.5x |
| `rewrite_source`, 20 queries | 849 us | 144 us | 5.9x |
| `strip_source`, 20 queries | 81 us | 278 us | 0.3x |
| `check_source`, 4 queries | 804 us | 551 us | 1.5x |

Parsing SQL is where Rust wins. Whole-file operations carry a fixed cost per
call in the Rust backend, so for small files they are on par with Python or
slower; `strip_source` is regex-only and faster in Python. Numbers vary
between machines and runs. Reproduce them with:

```bash
uv run python benchmarks/bench_core.py
```

## Parity

Both backends must give the same results. The guarantees are tested, not
assumed:

- [`tests/test_core_conformance.py`](https://github.com/jirikuncar/sqlty/blob/main/tests/test_core_conformance.py)
  runs one contract against every available backend: inferred types,
  rewrite and strip output, idempotency (`rewrite(rewrite(x)) == rewrite(x)`),
  round trips, and that string contents are never modified.
- CI runs the test suite with the Rust extension, and again without it
  (except the end-to-end tests).
- Tests compare the SQL type tables of both backends to catch drift.

### Known parity gaps

Two Rust gaps affect common code. Set `SQLTY_LIB=py` to work around them:

- **Query parameters.** The Rust backend infers nothing for a query that
  contains `%s` or `%(name)s` placeholders (the psycopg style), so
  `sqlty rewrite` leaves it unwrapped and `sqlty check` skips `stale-type`
  for it. Row type checks still run. The Python backend handles these
  placeholders for PostgreSQL, but not for MySQL.
- **MySQL.** MySQL requires whitespace after `--` for a comment, and the Rust
  parser follows that, so `--sql` followed by the query fails to parse with
  `dialect = "mysql"`: `rewrite` wraps nothing and `check` infers nothing.

Other known differences, and gaps both backends share, are tracked in
[PLAN-FUTURE.md](https://github.com/jirikuncar/sqlty/blob/main/PLAN-FUTURE.md#backend-parity-gaps).
In short:

- MySQL `INT3 UNSIGNED` / `MIDDLEINT UNSIGNED` columns parse in Python but
  make Rust skip the table.
- MySQL `ZEROFILL` columns make both backends skip the table.
- SQLite free-form type names (`UNSIGNED BIG INT`, `VARYING CHARACTER(255)`)
  make both backends skip the table.
- Rust reads dialects other than PostgreSQL, MySQL and SQLite as generic SQL;
  only those three are kept in parity.
