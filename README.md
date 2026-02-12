# SQLTy

> **Experimental** — APIs may change. Feedback welcome!

Type-safe SQL queries for Python. Infer types from SQL, rewrite source, lint annotations.

[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Powered by [sqlglot](https://sqlglot.com)** for SQL parsing across PostgreSQL, MySQL, and SQLite.

## The Problem

You write raw SQL in Python. The query returns columns, but Python has no idea what types they are:

```python
query = "SELECT id, name, created_at FROM users"
# What does this return? tuple[int, str, datetime]? Who knows.
```

## The Solution

Mark your SQL strings with `--sql\n`, add type casts, and let sqlty handle the rest:

```python
# 1. Write SQL with --sql prefix and type casts
query = "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
```

```bash
# 2. Run sqlty rewrite
sqlty rewrite .
```

```python
# 3. Result: type-safe SQL string
import datetime
from sqlty import SQL

query = SQL[tuple[int, str, datetime.datetime]](
    "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
)
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

Runtime type resolution handles import variations correctly — `from datetime import datetime` matches `TIMESTAMP` columns even though the AST name is just `datetime`.

## Schema-Based Inference

Skip explicit `::TYPE` casts by providing a schema file:

```toml
# pyproject.toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```python
# No casts needed — types inferred from schema
query = "--sql\nSELECT id, name, email FROM users"

# After rewrite:
query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

## Magic Codec

Import-time transformation — no `sqlty rewrite` step needed:

```python
# coding: sqlty

query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# At runtime: SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The codec is registered automatically when sqlty is installed. See [magic codec docs](docs/magic-codec.md).

## Lint Rules

| Rule | Default | Description |
|------|---------|-------------|
| `column-count-mismatch` | error | Annotation has different field count than query |
| `column-type-mismatch` | error | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | Annotation field name differs from column name |
| `select-star` | warn | `SELECT *` used (fixable: expand to explicit columns) |
| `select-star-no-schema` | error | `SELECT *` used but no schema available |
| `stale-rewrite` | warn | `--sql` strings not wrapped in `SQL[T](...)` |
| `stale-type` | warn | Type annotation doesn't match current inference |
| `inline-sql-comment` | warn | `"--sql SELECT ..."` makes query a SQL comment |

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
