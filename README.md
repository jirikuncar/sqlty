# SQLTy

[![Python 3.15+](https://img.shields.io/badge/python-3.15+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Your SQL already knows its types. SQLTy tells Python.

Type-safe raw SQL for Python. sqlty infers the row type of each query from
its SQL, writes it into your source as `SQL[T]`, and checks that it stays
correct.

**Documentation: [jirikuncar.github.io/sqlty](https://jirikuncar.github.io/sqlty/)**

> **Experimental.** APIs and configuration may change. Feedback welcome.

## How it works

Mark a raw SQL string with `--sql\n` and cast its columns. Running
`sqlty rewrite .` wraps it with the inferred row type:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[int, str, datetime.datetime]]("--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users")
```

Comment lines such as `# rewritten-file:` show the result; the test suite
checks them.

Each cast becomes a Python type: `INTEGER` is `int`, `TEXT` is `str`,
`TIMESTAMP` is `datetime.datetime`, `BYTEA` is `bytes` (see the
[type mapping](https://jirikuncar.github.io/sqlty/guide/writing-queries/#type-mapping)).
`SQL[T]` is a `str` subclass, so the query still works anywhere a string
does, and editors and type checkers now know its row type.

## Install

```bash
pip install sqlty    # or: uv add sqlty
```

`sqlty[psycopg]` adds psycopg 3 for the driver helpers. See
[Installation](https://jirikuncar.github.io/sqlty/installation/).

## Three commands

| Command | What it does |
|---------|--------------|
| `sqlty rewrite .` | Wrap `--sql` strings in `SQL[T](...)` with the inferred type |
| `sqlty check .` | Report drift, row type mismatches, `SELECT *` and unwrapped queries |
| `sqlty strip .` | Remove the wrappers, back to plain `--sql` strings |

`rewrite` and `strip` only add or remove the wrapper and its import; the
string itself is never modified. For a file of plain `--sql` strings,
`strip(rewrite(x)) == x`. Start with the
[Quickstart](https://jirikuncar.github.io/sqlty/quickstart/); to change a
typed query, see
[Editing queries](https://jirikuncar.github.io/sqlty/guide/writing-queries/#editing-queries).

## Features

### Row types

Use `TypedDict`, `NamedTuple`, dataclasses or Pydantic models instead of
tuples. `sqlty check` validates field count, types and names against the
query ([Row types guide](https://jirikuncar.github.io/sqlty/guide/row-types/)).
Here `id` is declared as `str` on purpose, and the mismatch is reported:

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Schema inference

Point sqlty at your `CREATE TABLE` statements and the casts become optional
([Schema inference guide](https://jirikuncar.github.io/sqlty/guide/schema-inference/)).
PostgreSQL, SQLite and MySQL are supported; with MySQL, the Rust backend has a
[known gap](https://jirikuncar.github.io/sqlty/reference/backends/#known-gaps).

```toml
# pyproject.toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255) NOT NULL);
```

```python
query = "--sql\nSELECT id, name, email FROM users"
# rewritten: query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

### More

- **[psycopg](https://jirikuncar.github.io/sqlty/guide/psycopg/).**
  `execute(conn, SQL[User](...))` returns a `Cursor[User]` with the matching
  row factory; `scalar` fetches one typed value.
- **[Magic codec](https://jirikuncar.github.io/sqlty/guide/magic-codec/).**
  Files declaring `# coding: sqlty` are rewritten when imported. Installing
  sqlty adds a `.pth` file that registers the codec at startup
  (`SQLTY_NO_MAGIC=1` turns it off); in an editable install of a sqlty
  checkout, register it with `import sqlty.magic`.
- **[Untyped queries](https://jirikuncar.github.io/sqlty/guide/untyped/).**
  With `untyped = "sentinel"`, queries sqlty cannot type become
  `SQL[UNTYPED]`, which type checkers refuse to treat as a concrete row.
- **[Generated stubs](https://jirikuncar.github.io/sqlty/guide/stubs/).**
  For a helper such as `sql("SELECT ...")`, decorate it with `@sqlty`,
  register it under `[[tool.sqlty.functions]]` in `pyproject.toml`, and run
  your code with `SQLTY_AUTO_GENERATE=1`. sqlty writes a `.pyi` stub with one
  overload per query.

## Use cases

- [Catch schema drift in CI](https://jirikuncar.github.io/sqlty/use-cases/#catch-schema-drift-in-ci)
- [Raw SQL without an ORM](https://jirikuncar.github.io/sqlty/use-cases/#raw-sql-without-an-orm)
- [Incremental adoption](https://jirikuncar.github.io/sqlty/use-cases/#incremental-adoption)
- [Scripts and notebooks](https://jirikuncar.github.io/sqlty/use-cases/#scripts-and-notebooks)
- [Typed data-access helpers](https://jirikuncar.github.io/sqlty/use-cases/#typed-data-access-helpers)
- [Guardrails for AI-generated SQL](https://jirikuncar.github.io/sqlty/use-cases/#guardrails-for-ai-generated-sql)

## Lint rules

`sqlty check` runs a set of rules with configurable severity;
`sqlty check --fix` applies the fixable ones and `sqlty rule NAME` explains
one. See the
[rules reference](https://jirikuncar.github.io/sqlty/reference/rules/).

## Backends

The CLI and `sqlty.core` use a Rust extension when available, with a
pure-Python fallback held to the same tests. See
[Backends](https://jirikuncar.github.io/sqlty/reference/backends/) for
[benchmarks](https://jirikuncar.github.io/sqlty/reference/backends/#benchmarks)
and how to choose one.

## Contributing

See [DEVELOPMENT.md](https://github.com/jirikuncar/sqlty/blob/main/DEVELOPMENT.md)
for setup, testing and contribution guidelines.

## License

MIT. See [LICENSE](https://github.com/jirikuncar/sqlty/blob/main/LICENSE).
