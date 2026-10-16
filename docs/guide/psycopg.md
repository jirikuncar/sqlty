# psycopg

`SQL[T]` is a `str` subclass, so it can be passed to any driver as is. For
[psycopg 3](https://www.psycopg.org/psycopg3/), `sqlty.drivers.psycopg` also
carries `T` through to the rows: the type checker sees `T`, and at runtime
the matching row factory builds `T`.

Install the optional dependency:

```bash
pip install "sqlty[psycopg]"
```

Full signatures are in the
[Python API reference](../reference/python-api.md#sqltydriverspsycopg).

## `execute`

`execute(conn, query, params=None)` runs `query: SQL[T]` and returns a
`psycopg.Cursor[T]`:

```python
from dataclasses import dataclass

import psycopg
from sqlty import SQL
from sqlty.drivers.psycopg import execute

@dataclass
class User:
    id: int
    name: str

query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE active = %s")

with psycopg.connect("dbname=app") as conn:
    users = execute(conn, query, (True,)).fetchall()  # list[User]
    first = execute(conn, query, (True,)).fetchone()  # User | None
```

`sqlty check` makes sure `User` matches the columns before the code runs
(see [Row types](row-types.md)); psycopg builds `User` objects at runtime.

The row factory is chosen from the query's runtime row type
([`row_form`](row-types.md#the-row-type-at-runtime)):

| `T` | Row factory |
|-----|-------------|
| `tuple[...]` | `tuple_row` |
| `TypedDict`, `dict[...]` | `dict_row` |
| `Protocol` | `namedtuple_row` |
| any other class (dataclass, `NamedTuple`, Pydantic model) | `class_row(T)` |

A query without a runtime row type, such as a plain string, yields tuples.

A [t-string query](writing-queries.md#t-strings), `SQLTemplate[T](t"...")`, runs
as a template, its values sent as parameters (psycopg 3.3 or later):

```python
from typing import TypedDict

import psycopg
from sqlty import SQLTemplate
from sqlty.drivers.psycopg import execute

class User(TypedDict):
    id: int
    name: str

user_id = 1
query = SQLTemplate[User](t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id}")

with psycopg.connect("dbname=app") as conn:
    user = execute(conn, query).fetchone()  # User | None
# error:
```

## `row_factory`

`row_factory(T)` returns the factory from the table above, for cursors you
create yourself. It raises `TypeError` for a type it cannot map.

```python
from sqlty import row_form
from sqlty.drivers.psycopg import row_factory

cursor = conn.cursor(row_factory=row_factory(row_form(query)))
users = cursor.execute(query, (True,)).fetchall()  # list[User]
```

## `scalar`

`scalar(conn, query, [form,] params=None)` returns the only column of the
first row. Its type comes from `query: SQL[tuple[T]]`:

```python
from sqlty import SQL
from sqlty.drivers.psycopg import scalar

email = SQL[tuple[str | None]]("--sql\nSELECT email::TEXT FROM users WHERE id = %s")
value = scalar(conn, email, (1,))  # str | None
```

For a plain string query, or to narrow the type, pass a `form`. It is a
[PEP 747](https://peps.python.org/pep-0747/) `TypeForm`, so unions and
`Literal` work where `type[T]` would not:

```python
from typing import Literal

role = scalar(conn, "--sql\nSELECT role::TEXT FROM users LIMIT 1", Literal["admin", "user"])
latest = scalar(conn, "--sql\nSELECT max(id)::INTEGER FROM users", int | None)
```

`form` is not checked at runtime. A query that returns no rows raises
`LookupError`, so `None` always means SQL `NULL`. A row with more than one
column raises `ValueError`.

## Without the helpers

Any psycopg API accepts `SQL[T]`. You lose only the typed rows:

```python
from psycopg.rows import dict_row

with conn.cursor(row_factory=dict_row) as cur:
    cur.execute(query, (True,))
    rows = cur.fetchall()  # list[dict[str, Any]]
```
