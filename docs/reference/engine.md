# Engine

sqlty's core operations (type inference, `SELECT *` expansion, rewrite,
strip, check and fix) run in one engine written in Rust: the `sqlty._core`
extension, built with [PyO3](https://pyo3.rs) and
[maturin](https://www.maturin.rs), parsing SQL with
[sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs). The CLI,
the [`sqlty.core`](python-api.md#sqltycore) functions, the
[magic codec](../guide/magic-codec.md)
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
`redshift`, `oracle`, ..., and `generic` for generic SQL). An unknown name is an
error that lists the valid ones: a typo such as `postgress` would otherwise read
queries as generic SQL. Type names are
spelled the PostgreSQL way, so `INT64` expands to `BIGINT` and `NUMBER` to
`DECIMAL`. PostgreSQL, MySQL and SQLite are specified by the test suite; the
others work on a best-effort basis. The `--dialect` CLI flag accepts the
first three; set others with `dialect` in `pyproject.toml`.

## PostgreSQL type resolution

PostgreSQL gives `CASE`, `COALESCE`, `GREATEST`/`LEAST`, `VALUES`, `ARRAY[...]`
and `UNION`/`INTERSECT`/`EXCEPT` one type for all their inputs, and picks an
operator by its operand types, by the rules of
[UNION, CASE and related constructs](https://www.postgresql.org/docs/17/typeconv-union-case.html)
and [operators](https://www.postgresql.org/docs/17/typeconv-oper.html). sqlty
follows them, and `tests/fixtures/pg_common_types.toml` checks it: each query
there has the type PostgreSQL 17 describes for its column, and
`tests/test_pg_common_types.py` compares sqlty's type with it, with no server
needed.

To add a query, put it in the fixture with any type, then fill in the types
from a disposable server:

```bash
SQLTY_PG_DSN=postgresql://postgres:postgres@localhost:5432/postgres \
    uv run --group psycopg python scripts/refresh_pg_types.py
```

With `SQLTY_PG_DSN` set, the test also checks the fixture against the server.

!!! note "Unknown, never a guess"
    A column sqlty can't resolve statically stays untyped. These stay untyped
    today: a bare `NULL` (`text` in PostgreSQL), arithmetic on an untyped string
    literal (`'1' + 1` is `integer`), a date minus a string literal, and the
    `|/` operator.

### Known gaps

A query whose type differs from PostgreSQL's is listed in `KNOWN_GAPS` in the
test, and here. None is listed today.

## Functions

A call to a built-in function is typed by the function's rules in the query's
dialect; an unknown function leaves its column untyped (see
[`unknown-function`](rules.md#unknown-function)). PostgreSQL follows its
"Functions and Operators" chapter. MySQL and SQLite type these:

| Group | MySQL | SQLite |
|---|---|---|
| Control flow | `IF`, `IFNULL` | `IIF`, `IFNULL` |
| Dates and times | `NOW`, `SYSDATE`, `UTC_TIMESTAMP`, `CURDATE`, `CURTIME`, `DATE`, `TIME`, `TIMESTAMP`, `STR_TO_DATE`, `FROM_UNIXTIME`, `CONVERT_TZ`, `LAST_DAY`, `MAKEDATE`, `MAKETIME`, `SEC_TO_TIME`, `TIMEDIFF`, `DATE_FORMAT`, `TIME_FORMAT`, `DAYNAME`, `MONTHNAME`, `YEAR`, `MONTH`, `DAY`, `HOUR`, `MINUTE`, `SECOND`, `WEEK`, `QUARTER`, `DATEDIFF`, `TIMESTAMPDIFF`, `TO_DAYS`, `TIME_TO_SEC`, ... | `date`, `time`, `datetime`, `strftime`, `timediff` (text), `julianday`, `unixepoch` |
| Strings | `CONCAT_WS`, `LPAD`, `RPAD`, `LEFT`, `RIGHT`, `REPEAT`, `REVERSE`, `LTRIM`, `RTRIM`, `UCASE`, `LCASE`, `HEX`, `MD5`, `SHA1`, `SHA2`, `FORMAT`, `QUOTE`, `SUBSTRING_INDEX`, `INSERT`, `ELT`, `UUID`, `LOCATE`, `INSTR`, `ASCII`, `FIELD`, `FIND_IN_SET`, ... | `ltrim`, `rtrim`, `hex`, `quote`, `printf`, `format`, `char`, `typeof`, `concat_ws`, `instr`, `unicode` |
| Numbers | `ABS`, `ROUND`, `TRUNCATE`, `CEIL`, `FLOOR`, `MOD`, `SIGN`, `SQRT`, `EXP`, `LN`, `LOG`, `POW`, trigonometry, `PI`, `RAND` | `abs`, `round`, `ceil`, `floor`, `trunc`, `mod`, `sign`, `random`, `total`, the math functions |
| JSON | `JSON_OBJECT`, `JSON_ARRAY`, `JSON_ARRAYAGG`, `JSON_OBJECTAGG`, `JSON_EXTRACT`, `JSON_SET`, ... (`JSON`), `JSON_UNQUOTE`, `JSON_TYPE`, `JSON_LENGTH`, `JSON_VALID`, `JSON_CONTAINS` | `json`, `json_object`, `json_array`, `json_group_array`, `json_set`, ... (text), `json_array_length`, `json_valid` |

The functions common to every dialect (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`,
`COALESCE`, `LENGTH`, `UPPER`, `LOWER`, `SUBSTR`, `REPLACE`, ...) are typed
everywhere. The full lists are in `rust/crates/sqlty-sql/src/analyzer/dialect_functions.rs`;
`tests/mdtest/mysql/functions.md` and `tests/mdtest/sqlite/functions.md` show
each group.

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

## Changes from the sqlglot backend

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
