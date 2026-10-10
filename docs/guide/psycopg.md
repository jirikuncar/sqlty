# psycopg 3

`SQL[T]` is a `str` subclass, so it works anywhere psycopg takes a query.
`sqlty.drivers.psycopg` goes further and picks the row factory from `T`: rows
come back as `T` at runtime and for the type checker. Install with
`pip install "sqlty[psycopg]"`.

## `execute`

```python
from dataclasses import dataclass

import psycopg

from sqlty import SQL
from sqlty.drivers.psycopg import execute


@dataclass
class User:
    id: int
    name: str
    email: str


query = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")

with psycopg.connect("dbname=app") as conn:
    users = execute(conn, query).fetchall()  # list[User]
# error:
```

`sqlty check` validates `User` against the query before the code runs;
psycopg would only fail at runtime.

| `T` | Row factory |
|-----|-------------|
| `tuple[...]` | `tuple_row` |
| `TypedDict`, `dict[...]` | `dict_row` |
| dataclass, `NamedTuple`, Pydantic model | `class_row(T)` |

`execute(conn, query, params=None)` returns a `Cursor[T]`. Only queries built
as `SQL[T](...)` carry `T` at runtime; plain strings fall back to `tuple_row`.

## `scalar`

`scalar(conn, query, [form,] params=None)` returns the single column of the
first row, typed from `SQL[tuple[T]]` or from an explicit
[PEP 747](https://peps.python.org/pep-0747/) `TypeForm`, so unions and
`Literal` work too:

```python
from typing import Literal

import psycopg

from sqlty import SQL
from sqlty.drivers.psycopg import scalar

with psycopg.connect("dbname=app") as conn:
    email = scalar(conn, SQL[tuple[str]]("--sql\nSELECT email::TEXT FROM users WHERE id = %s"), (1,))
    role = scalar(conn, "SELECT role FROM users LIMIT 1", Literal["admin", "user"])
# error:
```

No row raises `LookupError`, so `None` always means SQL `NULL`; more than one
column raises `ValueError`. `form` is not validated at runtime.

## Lower level

`sqlty.row_form(query)` returns `T` itself (`TypeError` if the query has no
runtime type, unless a default is given), and
`sqlty.drivers.psycopg.row_factory(T)` the matching psycopg factory for your
own cursors.
