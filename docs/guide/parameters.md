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
| named | `:name` | sqlite3 (`dialect = "sqlite"`) |

`%%` is a literal `%`. A placeholder inside a string literal or a comment is
text. In PostgreSQL `?` is also the JSONB operator, so it is a placeholder
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
| Known function | `lower(%s)`, `coalesce(age, %s)`, `lpad(name, %s, %s)` | the signature's types |

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
`param-count-mismatch` when the query has placeholders.

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
