# SQLTy

> **Experimental.** APIs and configuration may change. Feedback welcome.

[![Python 3.15+](https://img.shields.io/badge/python-3.15+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Documentation: [jirikuncar.github.io/sqlty](https://jirikuncar.github.io/sqlty/)**

Your SQL already knows its types. SQLTy tells Python.

## How it works

Mark a raw SQL string with `--sql\n` and cast its columns. Running
`sqlty rewrite .` wraps it with the inferred row type:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[int, str, datetime.datetime]]("--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users")
```

Each cast becomes a Python type: `INTEGER` is `int`, `TEXT` is `str`,
`TIMESTAMP` is `datetime.datetime` (see the
[type mapping](https://jirikuncar.github.io/sqlty/guide/writing-queries/#type-mapping)).
`SQL[T]` is a `str` subclass, so the query still works anywhere a string
does, and editors and type checkers now know its row type.

## Install

```bash
pip install sqlty    # or: uv add sqlty
```

`sqlty[psycopg]` adds psycopg 3 for the driver helpers. Wheels include a Rust
extension; without it sqlty falls back to pure Python. See
[Installation](https://jirikuncar.github.io/sqlty/installation/).

## Three commands

| Command | What it does |
|---------|--------------|
| `sqlty rewrite .` | Wrap `--sql` strings in `SQL[T](...)` with the inferred type |
| `sqlty check .` | Report drift, row type mismatches, `SELECT *` and unwrapped queries |
| `sqlty strip .` | Remove the wrappers, back to plain `--sql` strings |

The string itself is never modified, so `strip(rewrite(x)) == x`. Start with
the [Quickstart](https://jirikuncar.github.io/sqlty/quickstart/); to change a
typed query, see
[Editing queries](https://jirikuncar.github.io/sqlty/guide/writing-queries/#editing-queries).

## Features

### Row types

Use `TypedDict`, `NamedTuple`, dataclasses or Pydantic models instead of
tuples. `sqlty check` validates field count, types and names against the
query. [Row types guide](https://jirikuncar.github.io/sqlty/guide/row-types/).

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# check: [column-type-mismatch] "User.id is str but query column 'id' infers int"
```

### psycopg

`sqlty.drivers.psycopg.execute(conn, SQL[User](...))` picks the row factory
from `T` and returns a `Cursor[User]`; `scalar` fetches one typed value.
[psycopg guide](https://jirikuncar.github.io/sqlty/guide/psycopg/).

### Schema inference

Point sqlty at your `CREATE TABLE` statements and the casts become optional.
PostgreSQL, SQLite and MySQL are supported.
[Schema inference guide](https://jirikuncar.github.io/sqlty/guide/schema-inference/).

```toml
# pyproject.toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255));
```

```python
query = "--sql\nSELECT id, name, email FROM users"
# rewritten: query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

### Magic codec

Files that declare `# coding: sqlty` are rewritten as Python reads them, with
no rewrite step. Register the codec first by importing `sqlty.magic`, or with
a `.pth` file containing that import; installing the package does not
register it. Type checkers read the file on disk, so commit rewritten code.
[Magic codec guide](https://jirikuncar.github.io/sqlty/guide/magic-codec/).

```python
# coding: sqlty

query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Untyped queries

Queries sqlty cannot type stay plain strings by default. With
`untyped = "sentinel"` they become `SQL[UNTYPED]`, which type checkers refuse
to treat as a concrete row and `sqlty check` reports until casts or a schema
make the type known.
[Untyped queries guide](https://jirikuncar.github.io/sqlty/guide/untyped/).

```toml
# pyproject.toml
[tool.sqlty]
untyped = "sentinel"
```

```python
query = "--sql\nVALUES (1, 2)"
# rewritten: query = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

### Generated stubs

If your code sends queries through a helper such as `sql("SELECT ...")`,
decorate it with `@sqlty` and run your code with `SQLTY_AUTO_GENERATE=1`.
sqlty writes a `.pyi` stub with one overload per query, so each call site gets
its own return type.
[Generated stubs guide](https://jirikuncar.github.io/sqlty/guide/stubs/).

## Use cases

- [Catch schema drift in CI](https://jirikuncar.github.io/sqlty/use-cases/#catch-schema-drift-in-ci)
- [Raw SQL without an ORM](https://jirikuncar.github.io/sqlty/use-cases/#raw-sql-without-an-orm)
- [Incremental adoption](https://jirikuncar.github.io/sqlty/use-cases/#incremental-adoption)
- [Scripts and notebooks](https://jirikuncar.github.io/sqlty/use-cases/#scripts-and-notebooks)
- [Typed data-access helpers](https://jirikuncar.github.io/sqlty/use-cases/#typed-data-access-helpers)
- [Guardrails for AI-generated SQL](https://jirikuncar.github.io/sqlty/use-cases/#guardrails-for-ai-generated-sql)

## Lint rules

`sqlty check` has thirteen rules with configurable severity, among them
`column-type-mismatch` (row type field differs from its column),
`select-star-no-schema`, `stale-rewrite` (`--sql` string not wrapped) and
`stale-type` (annotation differs from the inferred type). See the
[rules reference](https://jirikuncar.github.io/sqlty/reference/rules/) and the
[configuration reference](https://jirikuncar.github.io/sqlty/reference/configuration/).

## Backends

Type inference runs in a Rust extension, with a pure-Python fallback held to
the same tests. Rust parses SQL much faster; the backends have some
[known gaps](https://jirikuncar.github.io/sqlty/reference/backends/#known-gaps).
See [Backends](https://jirikuncar.github.io/sqlty/reference/backends/) for
benchmarks and how to choose one.

## Contributing

See [DEVELOPMENT.md](https://github.com/jirikuncar/sqlty/blob/main/DEVELOPMENT.md) for setup, testing and contribution
guidelines.

## License

MIT. See [LICENSE](https://github.com/jirikuncar/sqlty/blob/main/LICENSE).
