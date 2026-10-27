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
