---
title: "Typed rustmy queries"
description: "Typed rows from the rustmy MySQL driver with sqlty.drivers.rustmy: the type checker sees SQL[T] rows, and the helpers build T at runtime, also on asyncio."
---

# rustmy

[rustmy](https://github.com/M4RC0Sx/rustmy) is a MySQL and MariaDB driver written
in Rust, with PyMySQL's `%s` and `%(name)s` placeholders. `SQL[T]` is a `str`
subclass, so rustmy runs it as is; `sqlty.drivers.rustmy` also carries `T` through
to the rows: the type checker sees `T`, and at runtime the helpers build `T`.

Install the optional dependency:

```bash
pip install "sqlty[rustmy]"
```

sqlty checks the queries with `dialect = "mysql"`, which reads `-- sql` as the
marker: MySQL takes `--` as a comment only when a space follows it.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(100) NOT NULL, email VARCHAR(255));
```

## `execute`

`execute(conn, query, params=None)` runs `query: SQL[T]` on a new cursor and
returns its rows as `T`:

```python
from dataclasses import dataclass

import rustmy
from sqlty import SQL
from sqlty.drivers.rustmy import execute


@dataclass
class User:
    id: int
    name: str


by_name = SQL[User]("-- sql\nSELECT id, name FROM users WHERE name = %s")

with rustmy.connect(host="localhost", database="app") as conn:
    users = execute(conn, by_name, ("ada",)).fetchall()  # list[User]
    first = execute(conn, by_name, ("ada",)).fetchone()  # User | None
# error:
```

`sqlty check` makes sure `User` matches the columns before the code runs (see
[Row types](row-types.md)), and that the call passes the query's parameters.
The rows are built from the query's runtime row type
([`row_form`](row-types.md#the-row-type-at-runtime)):

| `T` | Cursor | Row |
|-----|--------|-----|
| `tuple[...]` | `Cursor` | the tuple |
| `TypedDict`, `dict[...]` | `DictCursor` | the dict |
| `Protocol` | `Cursor` | a named tuple of the columns |
| any other class (dataclass, `NamedTuple`, Pydantic model) | `DictCursor` | `T(**row)` |

A query without a runtime row type, such as a plain string, yields tuples. The
returned `Rows[T]` has `fetchone`, `fetchmany`, `fetchall` and iteration; its
`cursor` is rustmy's, for `rowcount` and `lastrowid`.

With [typed parameters](parameters.md#typed-parameters), `SQL[T, P]`, pyright
and ty check `params` against `P`; `query.bind(params)` checks them on every type
checker:

```python
from typing import TypedDict

import rustmy
from sqlty import SQL
from sqlty.drivers.rustmy import execute, scalar


class ById(TypedDict):
    id: int


name_by_id = SQL[tuple[str], ById]("-- sql\nSELECT name FROM users WHERE id = %(id)s")

with rustmy.connect(host="localhost", database="app") as conn:
    row = execute(conn, name_by_id, {"id": 1}).fetchone()  # tuple[str] | None
    name = scalar(conn, name_by_id.bind({"id": 1}))  # str
# error:
```

## `scalar`

`scalar(conn, query, [form,] params=None)` returns the only column of the first
row, typed from `query: SQL[tuple[T]]` or from `form`, a
[PEP 747](https://peps.python.org/pep-0747/) `TypeForm`. A query that returns no
rows raises `LookupError`, so `None` always means SQL `NULL`.

```python
import rustmy
from sqlty.drivers.rustmy import scalar

with rustmy.connect(host="localhost", database="app") as conn:
    count = scalar(conn, "-- sql\nSELECT COUNT(*) FROM users", int)
```

## asyncio

`execute_async` and `scalar_async` take a `rustmy.aio` connection, such as one a
pool lends:

```python
from dataclasses import dataclass

from rustmy import aio
from sqlty import SQL
from sqlty.drivers.rustmy import execute_async


@dataclass
class User:
    id: int
    name: str


everyone = SQL[User]("-- sql\nSELECT id, name FROM users")


async def users(pool: aio.Pool) -> list[User]:
    async with pool.acquire() as conn:
        rows = await execute_async(conn, everyone)
        return await rows.fetchall()
# error:
```

## Without the helpers

Any rustmy cursor accepts `SQL[T]`, and `sqlty check` still checks its
parameters ([Driver calls](parameters.md#driver-calls)). You lose only the typed
rows.
