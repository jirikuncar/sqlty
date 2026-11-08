# Typed parameters

`SQL[R, P]`: `check` reads `R` as the row type, and `params-type-mismatch` compares `P`
with the parameters of the query.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, born DATE);
```

## Row type with positional parameters

```python
from sqlty import SQL

q = SQL[tuple[str], tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
# error:
```

## Row type with named parameters

```python
from typing import TypedDict

from sqlty import SQL


class ById(TypedDict):
    id: int


class User(TypedDict):
    id: str


# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User, ById]("--sql\nSELECT id, name FROM users WHERE id = %(id)s")
```

## A stale row type is fixed, its parameters kept

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
q = SQL[tuple[int], tuple[()]]("--sql\nSELECT name FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[str], tuple[()]]("--sql\nSELECT name FROM users")
```

## A multiline subscript

```python
from sqlty import SQL

q = SQL[
    tuple[str],
    tuple[int],
]("--sql\nSELECT name FROM users WHERE id = %s")
# error:
```

## `params-type-mismatch`

### A tuple with the wrong number of items

```python
from sqlty import SQL

# error: [params-type-mismatch] "tuple[int, int] has 2 items but the query has 1 parameter"
q = SQL[tuple[str], tuple[int, int]]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "tuple[()] has 0 items but the query has 1 parameter"
r = SQL[tuple[str], tuple[()]]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "tuple[int] has 1 item but the query has 0 parameters"
s = SQL[tuple[str], tuple[int]]("--sql\nSELECT name FROM users")
```

### `NoParams` for a query with parameters

```python
from sqlty import SQL, NoParams

# error: [params-type-mismatch] "NoParams takes none but the query has 1 parameter"
q = SQL[tuple[str], NoParams]("--sql\nSELECT name FROM users WHERE id = %s")
r = SQL[tuple[str], NoParams]("--sql\nSELECT name FROM users")
```

### A tuple for named parameters, a TypedDict for positional ones

```python
from typing import TypedDict

from sqlty import SQL


class ById(TypedDict):
    id: int


# error: [params-type-mismatch] "tuple[int] is a tuple but the query takes named parameters"
q = SQL[tuple[str], tuple[int]]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# error: [params-type-mismatch] "ById is a TypedDict but the query takes positional parameters"
r = SQL[tuple[str], ById]("--sql\nSELECT name FROM users WHERE id = %s")
```

### Keys that don't match the names

```python
from typing import NotRequired, TypedDict

from sqlty import SQL


class Wrong(TypedDict):
    user_id: int


class Optional(TypedDict):
    id: NotRequired[int]


# error: [params-type-mismatch] "Wrong has no key for query parameter 'id'"
# error: [params-type-mismatch] "Wrong.user_id is not a query parameter"
q = SQL[tuple[str], Wrong]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# error: [params-type-mismatch] "Optional.id is not required but query parameter 'id' is"
r = SQL[tuple[str], Optional]("--sql\nSELECT name FROM users WHERE id = %(id)s")
```

### Item types

`P` is judged as `param-type-mismatch` judges a value passed for it: `None` only for a
parameter stored in a `NOT NULL` column, and a `str` for anything, since the server
coerces text.

```python
import datetime
from typing import Optional, TypedDict

from sqlty import SQL


class NewUser(TypedDict):
    id: int | None
    name: str
    born: datetime.date | None


# error: [params-type-mismatch] "tuple[datetime.date] item 1 is datetime.date but query parameter 1 expects int"
q = SQL[tuple[str], tuple[datetime.date]]("--sql\nSELECT name FROM users WHERE id = %s")
r = SQL[tuple[str], tuple[Optional[int], str]]("--sql\nSELECT name FROM users WHERE id = %s AND name = %s")
# error: [params-type-mismatch] "NewUser.id is int | None but query parameter 'id' is not nullable"
new = SQL[tuple[int], NewUser]("--sql\nINSERT INTO users (id, name, born) VALUES (%(id)s, %(name)s, %(born)s) RETURNING id")
```

### What fits

`typing.Any`, a cast parameter, a type narrower than the query takes, and a `P`
written by `rewrite` are not reported.

```python
import typing
from typing import TypedDict

from sqlty import SQL


class ById(TypedDict):
    id: int


q = SQL[tuple[str], tuple[typing.Any]]("--sql\nSELECT name FROM users WHERE id = %s")
r = SQL[tuple[str], tuple[bytes]]("--sql\nSELECT name FROM users WHERE id = %s::INTEGER")
s = SQL[tuple[str], ById]("--sql\nSELECT name FROM users WHERE id = %(id)s")
t = SQL[tuple[str], tuple[int | None, str]]("--sql\nSELECT name FROM users WHERE id = %s OR name = %s")
u = SQL[tuple[str], typing.Any]("--sql\nSELECT name FROM users WHERE id = %s")
# error:
```

## Passed to a helper

A `SQL[R, P]` stored in a name and passed to a [registered helper](../../../docs/guide/helpers.md)
is checked as its query, and the arguments of the call against `P`, as a type checker
would. The helper needs no annotation.

```toml
[[tool.sqlty.functions]]
import-path = "pkg.db.fetch_one"
query-param = "query"
return-type-param = "form"

[[tool.sqlty.functions]]
import-path = "pkg.db.execute"
query-param = "query"
```

`pkg/db.py`:

```python
from typing import Any, LiteralString

async def fetch_one[T](conn: Any, query: LiteralString, form: type[T], /, **params: Any) -> T | None: ...
def execute(query: LiteralString, params: tuple[Any, ...]) -> None: ...
```

`pkg/queries.py`:

```python
from typing import TypedDict

from sqlty import SQL


class ById(TypedDict):
    id: int


BY_ID = SQL[tuple[str], ById]("--sql\nSELECT name FROM users WHERE id = %(id)s")
RENAME = SQL[tuple[()], tuple[str, int]]("--sql\nUPDATE users SET name = %s WHERE id = %s")
```

### In the same module

```python
from typing import Any, TypedDict

from sqlty import SQL
from pkg.db import execute, fetch_one


class ById(TypedDict):
    id: int


BY_ID = SQL[tuple[str], ById]("--sql\nSELECT name FROM users WHERE id = %(id)s")
RENAME = SQL[tuple[()], tuple[str, int]]("--sql\nUPDATE users SET name = %s WHERE id = %s")


async def run(conn: Any) -> None:
    await fetch_one(conn, BY_ID, tuple[str], id=1)
    await fetch_one(conn, BY_ID, tuple[str], id=True)
    # error: [param-type-mismatch] "parameter 'id' expects int, got str"
    await fetch_one(conn, BY_ID, tuple[str], id="not-an-int")
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    await fetch_one(conn, BY_ID, tuple[str])
    # error: [column-type-mismatch] "tuple[int] item 0 is int but query column 'name' infers str"
    await fetch_one(conn, BY_ID, tuple[int], id=1)
    execute(RENAME, ("ann", 1))
    # error: [param-type-mismatch] "parameter 2 expects int, got str"
    execute(RENAME, ("ann", "1"))
    # error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
    execute(RENAME, ("ann",))
```

### Imported

```python
from typing import Any

from pkg import queries
from pkg.db import execute, fetch_one
from pkg.queries import BY_ID


async def run(conn: Any) -> None:
    # error: [param-type-mismatch] "parameter 'id' expects int, got str"
    await fetch_one(conn, BY_ID, tuple[str], id="not-an-int")
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    await fetch_one(conn, BY_ID, tuple[str])
    # error: [param-type-mismatch] "parameter 1 expects str, got int"
    execute(queries.RENAME, (1, 1))
```

### `P` imported by the module of the query

`pkg/types.py`:

```python
from typing import TypedDict


class ByName(TypedDict):
    name: str
```

`pkg/by_name.py`:

```python
from sqlty import SQL
from pkg.types import ByName

BY_NAME = SQL[tuple[int], ByName]("--sql\nSELECT id FROM users WHERE name = %(name)s")
```

```python
from typing import Any

from pkg.by_name import BY_NAME
from pkg.db import fetch_one


async def run(conn: Any) -> None:
    # error: [param-type-mismatch] "parameter 'name' expects str, got int"
    await fetch_one(conn, BY_NAME, tuple[int], name=1)
```

### `P` that is not compared

A tuple with `...`, `typing.Any` and a type that is not a `TypedDict` leave the
arguments to the query, as for a string literal.

```python
import typing
from typing import Any

from sqlty import SQL
from pkg.db import execute, fetch_one

ANY = SQL[tuple[str], typing.Any]("--sql\nSELECT name FROM users WHERE id = %(id)s")
OPEN = SQL[tuple[()], tuple[int, ...]]("--sql\nUPDATE users SET name = %s WHERE id = %s")
PLAIN = "SELECT name FROM users WHERE id = %(id)s"


async def run(conn: Any) -> None:
    await fetch_one(conn, ANY, tuple[str], id="1")
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    await fetch_one(conn, ANY, tuple[str])
    execute(OPEN, ("ann", 1))
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    await fetch_one(conn, PLAIN, tuple[str])
```

### Without `preview`

A name bound to a `SQL[R](...)` call is resolved without query variants; one bound
to a plain string is not.

```toml
[tool.sqlty]
preview = false
```

```python
from typing import Any

from sqlty import SQL
from pkg.db import fetch_one
from pkg.queries import BY_ID

PLAIN = "SELECT name FROM users WHERE id = %(id)s"


async def run(conn: Any) -> None:
    # error: [param-type-mismatch] "parameter 'id' expects int, got str"
    await fetch_one(conn, BY_ID, tuple[str], id="not-an-int")
    await fetch_one(conn, PLAIN, tuple[str])
```

### Without `P`

`SQL[R]` has no `P`: the call is checked against the query, as a string literal is.

```python
from typing import Any

from sqlty import SQL
from pkg.db import fetch_one

BY_ID = SQL[tuple[str]]("--sql\nSELECT name FROM users WHERE id = %(id)s")


async def run(conn: Any) -> None:
    await fetch_one(conn, BY_ID, tuple[str], id="1")
    # error: [param-count-mismatch] "call is missing parameter 'id'"
    await fetch_one(conn, BY_ID, tuple[str])
```

### A local name is not the constant

```python
from typing import Any, LiteralString

from sqlty import SQL
from pkg.db import fetch_one

BY_ID = SQL[tuple[str]]("--sql\nSELECT name FROM users WHERE id = %(id)s")


async def run(conn: Any, BY_ID: LiteralString) -> None:
    await fetch_one(conn, BY_ID, tuple[str])
# error:
```
