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

## A literal percent sign beside a bound parameter

```python
from typing import TypedDict

from sqlty import SQL


class Row(TypedDict):
    n: int
    pct: str


q = SQL[Row, tuple[int]]("--sql\nSELECT %s::INTEGER AS n, '100%%'::TEXT AS pct")
# error:
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

### A `NoParams` of your own is a TypedDict

```python
from typing import TypedDict

from sqlty import SQL


class NoParams(TypedDict):
    id: int


q = SQL[tuple[str], NoParams]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# error:
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

### A key of a base in another module

Its type is not known here, so only the keys declared here are compared.

`pkg/base.py`:

```python
from typing import TypedDict


class Base(TypedDict):
    id: str
```

```python
from sqlty import SQL
from pkg.base import Base


class ByIdAndName(Base):
    name: int


# error: [params-type-mismatch] "ByIdAndName.name is int but query parameter 'name' expects str"
q = SQL[tuple[str], ByIdAndName]("--sql\nSELECT name FROM users WHERE id = %(id)s AND name = %(name)s")
```

### Through a type alias

`P` named by an alias is compared as the type the alias declares: `X = T`,
`X: TypeAlias = T` or `type X = T`, through other aliases and imports.

`pkg/aliases.py`:

```python
from typing import TypedDict


class ById(TypedDict):
    id: str


type Born = tuple[int]
Keyed = ById
```

`pkg/reexported.py`:

```python
from pkg.aliases import ById

Reexported = ById
```

```python
import datetime
from typing import TypeAlias

from sqlty import SQL
from pkg import aliases
from pkg.aliases import Born, Keyed
from pkg.reexported import Reexported

T = tuple[str | None, datetime.date]
Pair: TypeAlias = T
type Single = tuple[int]

# error: [params-type-mismatch] "T item 1 is str | None but query parameter 1 is not nullable"
q = SQL[None, T]("--sql\nUPDATE users SET name = %s WHERE born = %s")
# error: [params-type-mismatch] "Pair has 2 items but the query has 1 parameter"
r = SQL[tuple[str], Pair]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "Single item 1 is int but query parameter 1 expects str"
s = SQL[tuple[int], Single]("--sql\nSELECT id FROM users WHERE name = %s")
# error: [params-type-mismatch] "Born item 1 is int but query parameter 1 expects datetime.date"
t = SQL[tuple[int], Born]("--sql\nSELECT id FROM users WHERE born = %s")
# error: [params-type-mismatch] "aliases.Born item 1 is int but query parameter 1 expects datetime.date"
u = SQL[tuple[int], aliases.Born]("--sql\nSELECT id FROM users WHERE born = %s")
# error: [params-type-mismatch] "Keyed is a TypedDict but the query takes positional parameters"
v = SQL[tuple[str], Keyed]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "Reexported is a TypedDict but the query takes positional parameters"
w = SQL[tuple[str], Reexported]("--sql\nSELECT name FROM users WHERE id = %s")
```

### A name bound twice is not an alias

Nor is one a function or class binds where `P` is spelled.

```python
from sqlty import SQL

T = tuple[int]
T = tuple[int, int]

Pair = tuple[int, int]
if T:
    Pair = tuple[int]

Wide = tuple[int, int]


def run() -> None:
    Wide = tuple[int]
    w = SQL[tuple[str], Wide]("--sql\nSELECT name FROM users WHERE id = %s")


class Queries:
    Wide = tuple[int]
    w = SQL[tuple[str], Wide]("--sql\nSELECT name FROM users WHERE id = %s")


q = SQL[tuple[str], T]("--sql\nSELECT name FROM users WHERE id = %s")
p = SQL[tuple[str], Pair]("--sql\nSELECT name FROM users WHERE id = %s")
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

### `P` through an alias

A cast parameter takes any value, so only `P` says what it is.

```python
from sqlty import SQL
from pkg.db import execute

Rename = tuple[str, int]
RENAME = SQL[tuple[()], Rename]("--sql\nUPDATE users SET name = %s WHERE id = %s::INTEGER")


def run() -> None:
    execute(RENAME, ("ann", 1))
    # error: [param-type-mismatch] "parameter 2 expects int, got str"
    execute(RENAME, ("ann", "1"))
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

## A union `P` is checked member by member

Each member of a union `P` must fit the query: a type checker accepts any one of them.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from typing import Optional, TypedDict, Union

from sqlty import SQL, NoParams


class ById(TypedDict):
    id: int


type Params = tuple[bytes] | tuple[int]

# error: [params-type-mismatch] "tuple[bytes] item 1 is bytes but query parameter 1 expects int"
a = SQL[tuple[str], tuple[bytes] | tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "tuple[int, int] has 2 items but the query has 1 parameter"
b = SQL[tuple[str], Union[tuple[int], tuple[int, int]]]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "ById is a TypedDict but the query takes positional parameters"
c = SQL[tuple[str], tuple[int] | ById]("--sql\nSELECT name FROM users WHERE id = %s")
d = SQL[tuple[str], tuple[int] | tuple[str]]("--sql\nSELECT name FROM users WHERE id = %s")
e = SQL[tuple[int], NoParams]("--sql\nSELECT id FROM users")
# error: [params-type-mismatch] "None takes none but the query has 1 parameter"
f = SQL[tuple[str], Optional[tuple[int]]]("--sql\nSELECT name FROM users WHERE id = %s")
g = SQL[tuple[int], tuple[()] | None]("--sql\nSELECT id FROM users")
# error: [params-type-mismatch] "tuple[bytes] item 1 is bytes but query parameter 1 expects int"
h = SQL[tuple[str], Params]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "tuple[bytes] item 1 is bytes but query parameter 1 expects int"
i = SQL[tuple[str], Union[tuple[bytes], tuple[bytes]]]("--sql\nSELECT name FROM users WHERE id = %s")
# error: [params-type-mismatch] "NoParams takes none but the query has 1 parameter"
j = SQL[tuple[str], NoParams | tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
```
