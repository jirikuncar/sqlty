# Query parameters

A placeholder (`%s`, `%(name)s`, `$1`, `?`, `:name`) has no type of its own.
sqlty gives it one from where it sits in the query, and `sqlty check` compares
the arguments a call passes against it.

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER,
    born DATE
);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Placeholder styles

| Style | Spelling | Drivers |
|-------|----------|---------|
| format | `%s` | psycopg, MySQL drivers |
| pyformat | `%(name)s` | psycopg, MySQL drivers |
| numeric | `$1`, `$2` | asyncpg, PostgreSQL |
| qmark | `?`, `?1` | sqlite3, MySQL |
| named | `:name` | sqlite3, SQLAlchemy's `text()` (`dialect = "sqlite"` or `"postgres"`) |

`%%` is a literal `%`. A placeholder inside a string literal or a comment is
text. `:name` is read as SQLAlchemy reads it: `::int` casts, array slices such as
`a[1:2]`, and `:a::int` are not placeholders. In PostgreSQL `?` is also the JSONB operator, so it is a placeholder
only when the query does not parse otherwise.

## Where a parameter gets its type

| Context | Example | Parameter |
|---------|---------|-----------|
| Comparison | `age > %s` | the column's type (`int`) |
| `BETWEEN`, `IN (...)`, `IN (SELECT ...)` | `id IN (%s, %s)` | the other side's type |
| `LIKE` | `name LIKE %s` | `str` |
| `INSERT` and `UPDATE` | `INSERT INTO users (name) VALUES (%s)` | the target column's type |
| Cast | `%s::date`, `CAST(%s AS int)` | the cast type |
| `LIMIT`, `OFFSET`, `FETCH` | `LIMIT %s` | `int` |
| `= ANY(...)` | `id = ANY(%s)` | `list[int]` |
| Known function | `lower(%s)`, `coalesce(age, %s)`, `lpad(name, %s, %s)`, `pg_notify(%s, %s)` | the signature's types |
| Schema function | `finish_job(%s, %s)` with `CREATE FUNCTION finish_job(job_id bigint, ...)` | the declared argument types, when the overloads a call fits agree |

A `NOT NULL` target column also makes the parameter non-null, and a nullable
one nullable. Where the context says nothing (a bare `SELECT %s`, an unknown
table) the parameter stays untyped.

`sqlty.core.analyze_params` returns them:

```pycon
>>> from sqlty.core import analyze_params
>>> schema = "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, age INTEGER)"
>>> [(p.number, p.python_type, p.nullable) for p in analyze_params("INSERT INTO users VALUES (%s, %s, %s)", schema_sql=schema)]
[(1, 'int', False), (2, 'str', False), (3, 'int', True)]
```

## Checking calls

A call of a [marked helper](helpers.md) is checked when it spells out its
arguments. The helper takes them as a parameter named `params`, `parameters`
or `args` holding a tuple, list or dict literal, as `*args`, or as
`**params`:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...] = (), /) -> None: ...

execute("SELECT id FROM users WHERE id = %s AND age > %s", (1, 18))

# error: [param-type-mismatch] "parameter 2 expects int, got bytes"
execute("SELECT id FROM users WHERE id = %s AND age > %s", (1, b"1"))

# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
execute("SELECT id FROM users WHERE id = %s AND age > %s", (1,))
```

[`param-type-mismatch`](../reference/rules.md#param-type-mismatch) judges an
argument only where its type is stated: a literal, a variable annotated or
assigned a class, a call of a known class. An argument of unknown type is
never reported. Text (`str`) is accepted everywhere because the server coerces
it, `int`, `float` and `Decimal` are interchangeable, and an explicit cast
(`%s::text`) takes anything. `None` is reported only for a parameter stored in
a `NOT NULL` column. SQLite is dynamically typed, so with `dialect = "sqlite"`
only `None` and the parameter count are checked.

```python
from datetime import date
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...] = (), /) -> None: ...

def register(name: str, born: date, age: int | None, other) -> None:
    execute("INSERT INTO users (id, name, born) VALUES (%s, %s, %s)", (1, name, born))
    execute("INSERT INTO users (id, name) VALUES (%s, %s)", (1, other))
    # error: [param-type-mismatch] "parameter 3 expects datetime.date, got int"
    execute("INSERT INTO users (id, name, born) VALUES (%s, %s, %s)", (1, name, 5))
```

Named placeholders match a dict literal or keyword arguments;
[`param-count-mismatch`](../reference/rules.md#param-count-mismatch) reports
names the call does not pass; extra keys are ignored, as the drivers do:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def execute(query: Annotated[LiteralString, SQL], params: dict[str, object], /) -> None: ...
def run(query: Annotated[LiteralString, SQL], /, **params: object) -> None: ...

execute("DELETE FROM users WHERE id = %(id)s", {"id": 1})
# error: [param-count-mismatch] "call is missing parameter 'id'"
execute("DELETE FROM users WHERE id = %(id)s", {"ident": 1})
# error: [param-type-mismatch] "parameter 'age' expects int, got bytes"
run("DELETE FROM users WHERE age = %(age)s", age=b"1")
```

## Driver calls

`cursor.execute(query, params)`, `executemany` (each row), and
`connection.execute(...)` are checked on psycopg and sqlite3 receivers, and
`execute`, `fetch`, `fetchrow`, `fetchval` on asyncpg ones. The receiver must
be evident in the file: bound to `psycopg.connect(...)`, `sqlite3.connect(...)`,
`asyncpg.connect(...)`, `<connection>.cursor()` (also as `with ... as name`), or
annotated with a driver class. The query is a literal, `SQL[T]("...")` of one,
or a name assigned once to one. A call with no parameters is a
`param-count-mismatch` when the query has placeholders, and a single value
passed as the parameters, `execute(query, user_id)`, is a `param-type-mismatch`:
the driver wants a sequence, `(user_id,)`.

```python
import sqlite3

conn = sqlite3.connect("app.db")
# error: [param-count-mismatch] "query has 1 parameter, the call passes 0"
conn.execute("SELECT id FROM users WHERE id = ?")
```

asyncpg-style `*args` numbers its placeholders `$1`, `$2`, ...:

```python
from typing import Annotated, LiteralString
from sqlty import SQL

async def fetch(query: Annotated[LiteralString, SQL], *args: object) -> list[object]: ...

async def main() -> None:
    await fetch("SELECT id FROM users WHERE id = $1 AND name = $2", 1, "a")
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    await fetch("SELECT id FROM users WHERE id = $1 AND name = $2", 1)
```

## Typed parameters

`SQL[R, P]` types the parameters of a query as `P`, next to its rows `R`. Your
type checker then checks the values you pass, without running `sqlty check`:

```python
from typing import TypedDict

from sqlty import SQL


class ById(TypedDict):
    id: int


by_id = SQL[tuple[str], ById]("--sql\nSELECT name FROM users WHERE id = %(id)s")
by_name = SQL[tuple[int], tuple[str]]("--sql\nSELECT id FROM users WHERE name = %s")
count = SQL[tuple[int], tuple[()]]("--sql\nSELECT count(*) FROM users")

by_id.bind({"id": 1})
by_name.bind(("alice",))
count.bind(())
```

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str | None]] should be SQL[tuple[str]]"
by_id = SQL[tuple[str | None], tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
```

Positional parameters are a `tuple`, one item per placeholder. Named
parameters are a `TypedDict`, one key per name. A query without parameters
takes `tuple[()]`.

`query.bind(params)` checks `params` against `P` and returns a `Bound[R]`, a
`(query, params)` tuple. `cursor.execute(*bound)` runs it, and the
[psycopg helpers](psycopg.md) take it in place of the query.

`P` defaults to `Any`, so `SQL[R]` takes any parameters and every
`SQL[R]` you already have stays valid. A query is typed like a function from
`P` to `R`: a query that takes `str | None` fits where one that takes `str` is
expected, not the other way around.

`sqlty check` ignores `P`: it compares `R` with the query, and the parameters
of each call with the query, as in [Checking calls](#checking-calls). Its fixes
rewrite `R` and keep `P`.

### What each type checker checks

| Call | mypy | pyright | ty |
|------|------|---------|----|
| `query.bind(params)` | yes | yes | yes |
| `execute(conn, query, params)` | no | yes | yes |
| `scalar(conn, query, params)` | no | no | no |

!!! note "Use `bind` to check `params` on mypy"
    mypy cannot infer `P` from a dict literal for a `TypedDict`
    ([python/mypy#19201](https://github.com/python/mypy/issues/19201)), so
    `execute` keeps a signature that takes any parameters on mypy. `scalar`
    keeps it on every checker. `execute(conn, query.bind(params))` and
    `scalar(conn, query.bind(params))` check `params` everywhere.
