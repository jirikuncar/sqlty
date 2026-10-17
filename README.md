# SQLTy

[![PyPI](https://img.shields.io/pypi/v/sqlty.svg)](https://pypi.org/project/sqlty/)
[![Python 3.14+](https://img.shields.io/badge/python-3.14+-blue.svg)](https://www.python.org/downloads/)
[![Tests](https://github.com/jirikuncar/sqlty/actions/workflows/tests.yml/badge.svg)](https://github.com/jirikuncar/sqlty/actions/workflows/tests.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Type-safe raw SQL for Python. Keep your queries as plain SQL strings; sqlty
reads their row types from the SQL and hands them to your type checker, so a
migration that changes a column is caught in CI, not in production.

<p align="center">
  <img src="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/demo.gif" alt="A terminal session: sqlty rewrite adds the row type to a query, a migration makes email nullable, sqlty check --fix updates the type and mypy reports the line that would crash" width="720">
  <br>
  <em>A migration makes <code>email</code> nullable. sqlty and mypy find the code that would crash.</em>
  (<a href="https://github.com/jirikuncar/sqlty/blob/main/docs/assets/demo.tape">view source</a>)
</p>

**Documentation: [jirikuncar.github.io/sqlty](https://jirikuncar.github.io/sqlty/)**

> **Beta.** APIs and configuration may change before 1.0. Feedback welcome in
> [GitHub Issues](https://github.com/jirikuncar/sqlty/issues).

## The problem

Drivers return rows of `Any`. Nothing ties the columns a query selects to the
code that reads them:

```python
row = cur.execute("SELECT id, email FROM users WHERE id = %s", (user_id,)).fetchone()
row[1].lower()  # works until a migration makes email nullable
```

A renamed, retyped or reordered column surfaces at runtime. An ORM fixes this
by replacing SQL with its own query API; hand-written annotations drift from
the SQL without anyone noticing.

## How sqlty solves it

Mark a query with `--sql\n` (`-- sql\n` for MySQL) and run `sqlty rewrite .`.
It wraps the string with the row type inferred from the SQL:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[int, str, datetime.datetime]]("--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users")
```

Comment lines such as `# rewritten-file:` show the result; the test suite
checks them.

`SQL[T]` is a `str` subclass: the query still runs anywhere a string does, you
can still copy it into `psql`, and mypy, pyright and ty now know its row type.
Casts map to Python types (`INTEGER` is `int`, `TIMESTAMP` is
`datetime.datetime`; see the
[type mapping](https://jirikuncar.github.io/sqlty/guide/writing-queries/#type-mapping)).

### No casts with a schema

Point sqlty at your `CREATE TABLE` statements, and it types plain SQL. It
reads them offline: no database connection, in development or in CI.
PostgreSQL, SQLite and MySQL are supported, and other
[dialects](https://jirikuncar.github.io/sqlty/reference/engine/#dialects) on a
best-effort basis
([Schema inference guide](https://jirikuncar.github.io/sqlty/guide/schema-inference/)).

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

## What it catches

### A migration changes a column

```toml
[tool.sqlty]
schema = "schema.sql"
```

`email` used to be `NOT NULL`; the migration in `schema.sql` dropped the
constraint. The annotation written before it no longer holds:

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);
```

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int, str]] should be SQL[tuple[int, str | None]]"
query = SQL[tuple[int, str]]("--sql\nSELECT id, email FROM users")
```

`sqlty check --fix .` writes the new type, and your type checker reports every
`row[1].lower()` that now needs a `None` check. For rolling deploys,
[`sqlty breaking --against origin/main`](https://jirikuncar.github.io/sqlty/guide/breaking-changes/)
checks old code against the new schema and new code against the old one.

### A row type that doesn't match its query

Use `TypedDict`, `NamedTuple`, dataclasses or Pydantic models instead of
tuples, and `sqlty check` validates field count, types and names against the
query ([Row types guide](https://jirikuncar.github.io/sqlty/guide/row-types/)):

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Plausible SQL from a code assistant

An agent writes a query whose columns don't match the code that consumes it.
Run `sqlty check` in a pre-commit hook or CI and it fails before review, with
one `file:line: [rule] message` line per problem that the agent can act on
([Guardrails for AI-generated SQL](https://jirikuncar.github.io/sqlty/use-cases/#guardrails-for-ai-generated-sql),
[sqlty skill](https://jirikuncar.github.io/sqlty/recipes/agents/)).

## Get started

```bash
pip install sqlty    # or: uv add sqlty
```

| Command | What it does |
|---------|--------------|
| `sqlty rewrite .` | Wrap `--sql` strings in `SQL[T](...)` with the inferred type |
| `sqlty check .` | Report drift, row type mismatches, `SELECT *` and unwrapped queries |
| `sqlty strip .` | Remove the wrappers, back to plain `--sql` strings |

`rewrite` and `strip` only add or remove the wrapper and its import, never the
string, so adopting sqlty is reversible: for a file of plain `--sql` strings,
`strip(rewrite(x)) == x`. Convert one module at a time; `sqlty check` lists
the prefixed strings not yet wrapped.

Next: the [Quickstart](https://jirikuncar.github.io/sqlty/quickstart/),
[CI setup](https://jirikuncar.github.io/sqlty/recipes/ci/) and
[migrating an existing codebase](https://jirikuncar.github.io/sqlty/recipes/migration/).

## Works with

- **[psycopg](https://jirikuncar.github.io/sqlty/guide/psycopg/).**
  `execute(conn, SQL[User](...))` returns a `Cursor[User]` with the matching
  row factory; `scalar` fetches one typed value. Install `sqlty[psycopg]`.
- **[Magic codec](https://jirikuncar.github.io/sqlty/guide/magic-codec/).**
  Scripts and notebook helpers declaring `# coding: sqlty` are rewritten when
  imported, with no rewrite step. Installing sqlty adds a `.pth` file that
  registers the codec at startup (`SQLTY_NO_MAGIC=1` turns it off); in an
  editable install of a sqlty checkout, register it with `import sqlty.magic`.
- **[Untyped queries](https://jirikuncar.github.io/sqlty/guide/untyped/).**
  With `untyped = "sentinel"`, queries sqlty cannot type become
  `SQL[UNTYPED]`, which type checkers refuse to treat as a concrete row.
- **[Lint rules](https://jirikuncar.github.io/sqlty/reference/rules/).**
  Configurable severity, `sqlty check --fix` for the fixable ones and
  `sqlty rule NAME` to explain one.

## When to use something else

- **An ORM** (SQLAlchemy, Django): use its typing support.
- **SQL built at runtime**: sqlty only reads string literals.
- **Runtime validation**: sqlty is static analysis; use Pydantic to validate
  rows as they arrive.

[Why SQLTy?](https://jirikuncar.github.io/sqlty/motivation/) compares it with
sqlc, sqlx, PgTyped and other typed-SQL tools.

## Engine

SQL parsing, type inference and source rewriting run in a Rust engine
([sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs) via
[PyO3](https://pyo3.rs)); wheels cover Linux, macOS and Windows. See
[Engine](https://jirikuncar.github.io/sqlty/reference/engine/) for dialects and
[benchmarks](https://jirikuncar.github.io/sqlty/reference/engine/#benchmarks).

## Contributing

See [CONTRIBUTING.md](https://github.com/jirikuncar/sqlty/blob/main/CONTRIBUTING.md)
and [DEVELOPMENT.md](https://github.com/jirikuncar/sqlty/blob/main/DEVELOPMENT.md).

## AI disclosure

SQLTy is developed with AI coding agents, mainly Claude Code. Every change is
reviewed by a maintainer and covered by the test suite; commits written with an
agent carry a `Co-Authored-By` trailer. Contributors follow the
[AI-assisted contributions](https://github.com/jirikuncar/sqlty/blob/main/CONTRIBUTING.md#ai-assisted-contributions)
policy.

## Credits

The projects sqlty is built from, tested with and compared against, with their
licenses, are in [Credits](https://jirikuncar.github.io/sqlty/credits/).

## License

MIT. See [LICENSE](https://github.com/jirikuncar/sqlty/blob/main/LICENSE).
