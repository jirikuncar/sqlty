# SQLTy

> **Experimental** — APIs may change. Feedback welcome!

Type-safe SQL queries for Python. Infer types from SQL, rewrite source, lint annotations.

[![Python 3.15+](https://img.shields.io/badge/python-3.15+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Powered by [sqlglot](https://sqlglot.com)** for SQL parsing across PostgreSQL, MySQL, and SQLite.

## The Problem

You write raw SQL in Python. The query returns columns, but Python has no idea what types they are:

```python
query = "SELECT id, name, created_at FROM users"
# What does this return? tuple[int, str, datetime]? Who knows.
```

## The Solution

Mark your SQL strings with `--sql\n`, add type casts, and run `sqlty rewrite .`:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[int, str, datetime.datetime]]("--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users")
```

`SQL[T]` is a `str` subclass — it works everywhere a string works, but now your types are visible to editors, type checkers, and linters.

## Install

```bash
pip install sqlty
# or
uv add sqlty
```

## Quick Start

### 1. Rewrite — add type annotations

```bash
sqlty rewrite .          # "--sql\n..." → SQL[T]("--sql\n...")
```

### 2. Check — lint for type issues

```bash
sqlty check .            # catch mismatches, SELECT *, stale types
```

### 3. Strip — remove annotations (to edit SQL)

```bash
sqlty strip .            # SQL[T]("--sql\n...") → "--sql\n..."
```

| Command | What it does | Analogy |
|---------|-------------|---------|
| `sqlty rewrite .` | Add type annotations to `--sql` strings | `ruff format` |
| `sqlty strip .` | Remove annotations back to plain strings | undo rewrite |
| `sqlty check .` | Lint: type mismatches, SELECT *, stale types | `ruff check` |

## User-Defined Types

Beyond `tuple[...]`, sqlty validates `TypedDict`, `NamedTuple`, `dataclass`, and Pydantic `BaseModel` types against query columns:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# sqlty check validates: field count, types, and names match the query
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Mismatches produce clear diagnostics:

```
error[column-type-mismatch]: User.id is str but query column 'id' infers int
error[column-count-mismatch]: User has 3 fields but query returns 2 columns
warn[column-name-mismatch]: User.user_id doesn't match query column 'id'
```

`ReadOnly`, `Required` and `NotRequired` are looked through. Closed TypedDicts (PEP 728: `closed=True`, `extra_items=Never`, or a subclass of either) match columns by key: `NotRequired` keys may be absent, and extra columns are allowed only through `extra_items`, whose type they must match. Undeclared columns and missing required keys are `typeddict-key-mismatch` errors.:

```python
from typing import NotRequired, ReadOnly, TypedDict
from sqlty import SQL

class Metrics(TypedDict, extra_items=int):
    day: ReadOnly[str]
    note: NotRequired[str]

query = SQL[Metrics]("--sql\nSELECT day::TEXT, views::INTEGER, ratio::FLOAT FROM stats")
# error: [column-type-mismatch] "Metrics.ratio is extra_items int but query column 'ratio' infers float"
```

Runtime type resolution handles import variations correctly — `from datetime import datetime` matches `TIMESTAMP` columns even though the AST name is just `datetime`.

### TypedDict inheritance

Subclasses must respect a base's openness (PEP 728). A closed base admits no new keys, `extra_items`, or `closed=False`. A base with mutable `extra_items` must be kept exactly: no `closed=True`, `extra_items=Never`, `ReadOnly` or other type, and new keys must be `NotRequired` and of that type. A base with `ReadOnly` `extra_items` may be narrowed or closed, and new keys must be assignable to it. Keys a sibling base brings in count as new. Violations are `typeddict-inheritance` errors on the subclass's `class` line; a rejected override keeps the base's openness. Only same-file bases spelled as bare names (not generic or imported) are checked, and types are compared only when the answer is certain (plain classes and their unions):

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class User(Base):
    name: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
```

## psycopg3 Integration

`SQL[T]` is a `str` subclass, so it works anywhere psycopg3 takes a query. `sqlty.drivers.psycopg.execute` goes further: it picks the row factory from `T`, so rows come back as `T` both at runtime and for the type checker:

```python
import psycopg
from dataclasses import dataclass
from sqlty import SQL
from sqlty.drivers.psycopg import execute

@dataclass
class User:
    id: int
    name: str
    email: str

# sqlty check: validates (int, str, str) matches User fields at dev time
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")

with psycopg.connect("dbname=mydb") as conn:
    users = execute(conn, query).fetchall()  # list[User]
```

| `T` | Row factory |
|-----|-------------|
| `tuple[...]` | `tuple_row` |
| `TypedDict`, `dict[...]` | `dict_row` |
| dataclass, `NamedTuple`, Pydantic model | `class_row(T)` |

`sqlty.row_form(query)` returns `T` itself, and `row_factory(T)` the matching psycopg factory for use with your own cursors. Only queries built as `SQL[T](...)` carry `T` at runtime; `execute` falls back to tuples otherwise.

`scalar` fetches a single value, typed from `SQL[tuple[T]]` or from any type expression, unions and `Literal` included (PEP 747 `TypeForm`). No row raises `LookupError`, so `None` always means SQL `NULL`:

```python
email = scalar(conn, SQL[tuple[str | None]]("--sql\nSELECT email::TEXT FROM users WHERE id = %s"), (1,))  # str | None
role = scalar(conn, "--sql\nSELECT role::TEXT FROM users LIMIT 1", Literal["admin", "user"])
```

**Without sqlty**, if the SQL drifts from the Python type (column added, type changed, name mismatch), you get a runtime error. **With sqlty**, `sqlty check` catches the mismatch before the code runs.

## Schema-Based Inference

Skip explicit `::TYPE` casts by providing a schema file:

```toml
# pyproject.toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

No casts needed — types inferred from schema:

```python
query = "--sql\nSELECT id, name, email FROM users"
# rewritten: query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

## Magic Codec

Import-time transformation — no `sqlty rewrite` step needed:

```python
# coding: sqlty

query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The codec is registered automatically when sqlty is installed. See [magic codec docs](docs/guide/magic-codec.md).

## Untyped Queries

Queries that return rows sqlty can't type (`VALUES`, unparseable SQL) are
left as plain strings by default. Opt in to mark them explicitly with the
`UNTYPED` sentinel instead:

```toml
# pyproject.toml
[tool.sqlty]
untyped = "sentinel"  # or: sqlty rewrite --untyped sentinel
```

```python
query = "--sql\nVALUES (1, 2)"
# rewritten: query = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

`UNTYPED` is a [PEP 661](https://peps.python.org/pep-0661/) sentinel, not
`Any`: mypy, pyright and ty reject using its rows as a concrete type, and
`sqlty check` reports it as `untyped-query` until casts or a schema make the
row type known. Statements that return no rows (DDL, DML without `RETURNING`)
stay unwrapped.

The same setting applies to `.pyi` stubs generated for `@sqlty` functions
(`SQLTY_AUTO_GENERATE=1`): uninferable queries get a `SQL[UNTYPED]` overload,
and the catch-all `(query: str)` overload returns `SQL[UNTYPED]` (or the
function's `return_type_template` applied to `UNTYPED`) instead of `str`, so
a call missing from a stale stub is a type error rather than silently untyped.

Typed overloads that overlap the `SQL[UNTYPED]` catch-all carry a per-line
`# type: ignore[overload-overlap]`, so other overlaps in the stub are still
reported. pyright reports only the first such overlap per catch-all, so with
`reportUnnecessaryTypeIgnoreComment` enabled it flags the remaining ignores;
with `enableTypeIgnoreComments = false` it reports the overlaps instead.
Incremental stub updates add new overloads and replace existing ones whose
return type changed (including hand edits), so switching `untyped` mode
rewrites `SQL[Any]`/`SQL[UNTYPED]` overloads on the next update; until then
a stub check reports them as stale.

## Lint Rules

| Rule | Default | Description |
|------|---------|-------------|
| `column-count-mismatch` | error | Annotation has different field count than query |
| `column-type-mismatch` | error | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | Annotation field name differs from column name |
| `typeddict-key-mismatch` | error | Query columns don't fit a closed / `extra_items` TypedDict's keys |
| `typeddict-inheritance` | error | TypedDict subclass changes openness or adds keys a closed / `extra_items` base forbids (PEP 728) |
| `select-star` | warn | `SELECT *` used (fixable: expand to explicit columns) |
| `select-star-no-schema` | error | `SELECT *` used but no schema available |
| `stale-rewrite` | warn | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | Type annotation doesn't match current inference |
| `inline-sql-comment` | warn | `"--sql SELECT ..."` makes query a SQL comment |
| `fstring-skipped` | warn | f-string with `--sql` skipped (cannot be statically rewritten) |
| `tstring-skipped` | warn | t-string with `--sql` skipped (cannot be statically rewritten) |
| `untyped-query` | warn | `SQL[UNTYPED]` query has no inferable row type |

Configure in `pyproject.toml`:

```toml
[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

## Benchmarks

Optional Rust extension (PyO3/maturin) accelerates all core operations. Pure-Python fallback used automatically when not available.

| Benchmark | Python | Rust | Speedup |
|-----------|-------:|-----:|--------:|
| `infer_type`: 1 column | 120 us | 5 us | **24x** |
| `infer_type`: 4 columns | 340 us | 12 us | **28x** |
| `rewrite_source`: 3 queries | 600 us | 25 us | **24x** |
| `check_source`: 4 queries | 415 us | 24 us | **17x** |

**Geometric mean: ~15x** (Linux x86_64). `strip_source` is regex-only, so both backends perform similarly.

```bash
uv run python benchmarks/bench_core.py
```

## Testing

Markdown-based test framework (mdtest) inspired by [ty's mdtest](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md):

```bash
make test            # all tests
make test-mdtest     # mdtests only
make test-unit       # unit tests only
```

Browse `tests/mdtest/` to see behavior specified as readable markdown test suites.

## Documentation

- **[User Guide](docs/index.md)** — complete guide with examples
- **[API Reference](docs/api.md)** — CLI, types, configuration, lint rules
- **[Usage Patterns](docs/patterns.md)** — rewrite/strip, magic codec, schema inference
- **[Advanced Usage](docs/advanced-usage.md)** — dialects, multiple schemas, pre-commit

## Contributing

See **[DEVELOPMENT.md](DEVELOPMENT.md)** for setup, testing, and contribution guidelines.

## License

MIT — see [LICENSE](LICENSE).
