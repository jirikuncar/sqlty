# Parameter types

`rewrite` writes the type of the parameters of each query, `SQL[R, P]`: a `tuple` for
positional parameters, and a generated `TypedDict` for named ones.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL, email TEXT, born DATE);
```

## Positional parameters

A parameter is `T | None` unless a `NOT NULL` column stores it.

```python
q = "--sql\nSELECT name FROM users WHERE id = %s AND born > %s"
r = "--sql\nINSERT INTO users (id, name, email) VALUES (%s, %s, %s)"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[str], tuple[int | None, datetime.date | None]]("--sql\nSELECT name FROM users WHERE id = %s AND born > %s")
# rewritten-file: r = "--sql\nINSERT INTO users (id, name, email) VALUES (%s, %s, %s)"
```

## No parameters

A query without parameters keeps `SQL[R]`: `P` defaults to `Any`.

```python
q = "--sql\nSELECT name FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[str]]("--sql\nSELECT name FROM users")
```


## A `tuple[()]` you wrote is kept

```python
from sqlty import SQL

q = SQL[tuple[str], tuple[()]]("--sql\nSELECT name FROM users")
count = SQL[tuple[int], tuple[()]]("--sql\nSELECT count(*) FROM users")
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], tuple[()]]("--sql\nSELECT name FROM users")
# rewritten-file: count = SQL[tuple[int], tuple[()]]("--sql\nSELECT count(*) FROM users")
```

## Named parameters

```python
by_id = "--sql\nSELECT name FROM users WHERE id = %(id)s OR email = %(email)s"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class ByIdParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:     email: str | None
# rewritten-file:
# rewritten-file:
# rewritten-file: by_id = SQL[tuple[str], ByIdParams]("--sql\nSELECT name FROM users WHERE id = %(id)s OR email = %(email)s")
```

## A constant's class is named in Pascal case

```python
BY_EMAIL = "--sql\nSELECT id FROM users WHERE email = %(email)s"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class ByEmailParams(typing.TypedDict):
# rewritten-file:     email: str | None
# rewritten-file:
# rewritten-file:
# rewritten-file: BY_EMAIL = SQL[tuple[int], ByEmailParams]("--sql\nSELECT id FROM users WHERE email = %(email)s")
```

## Named parameters outside an assignment

```python
def run(query: str) -> None: ...


run("--sql\nSELECT name FROM users WHERE id = %(id)s")
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class QueryParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: def run(query: str) -> None: ...
# rewritten-file:
# rewritten-file:
# rewritten-file: run(SQL[tuple[str], QueryParams]("--sql\nSELECT name FROM users WHERE id = %(id)s"))
```

## A parenthesized query names its class

```python
q = (
    "--sql\nSELECT name FROM users WHERE id = %(id)s"
)
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class QParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: q = (
# rewritten-file:     SQL[tuple[str], QParams]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# rewritten-file: )
```

## Each named query gets its own class

```python
by_id = "--sql\nSELECT name FROM users WHERE id = %(id)s"
by_id_again = "--sql\nSELECT email FROM users WHERE id = %(id)s"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class ByIdParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class ByIdAgainParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: by_id = SQL[tuple[str], ByIdParams]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# rewritten-file: by_id_again = SQL[tuple[str | None], ByIdAgainParams]("--sql\nSELECT email FROM users WHERE id = %(id)s")
```

## A name already taken gets a number

```python
QParams = None
q = "--sql\nSELECT name FROM users WHERE id = %(id)s"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class QParams2(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: QParams = None
# rewritten-file: q = SQL[tuple[str], QParams2]("--sql\nSELECT name FROM users WHERE id = %(id)s")
```

## A class it generated before is reused

```python
import typing

from sqlty import SQL


# sqlty: generated
class QParams(typing.TypedDict):
    id: int | None


q = SQL[tuple[str]]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# rewritten-file: import typing
# rewritten-file:
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class QParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], QParams]("--sql\nSELECT name FROM users WHERE id = %(id)s")
```

## A keyword parameter takes the functional `TypedDict` syntax

```python
q = "--sql\nSELECT name FROM users WHERE id = %(from)s"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: QParams = typing.TypedDict('QParams', {'from': int | None})
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], QParams]("--sql\nSELECT name FROM users WHERE id = %(from)s")
```

## Unknown and cast parameters are `typing.Any`

```python
q = "--sql\nSELECT name, %s::INTEGER AS n FROM users WHERE %s IS NULL"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[str, int | None], tuple[typing.Any, typing.Any]]("--sql\nSELECT name, %s::INTEGER AS n FROM users WHERE %s IS NULL")
```

## An existing wrapper gets its parameters

```python
from sqlty import SQL

q = SQL[tuple[str]]("--sql\nSELECT name FROM users WHERE id = %s")
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], tuple[int | None]]("--sql\nSELECT name FROM users WHERE id = %s")
```

## Written parameters are kept

```python
from sqlty import SQL

q = SQL[tuple[str], tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
```

## `check --fix` writes them too

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
q = SQL[tuple[int]]("--sql\nSELECT name FROM users WHERE id = %s")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[str], tuple[int | None]]("--sql\nSELECT name FROM users WHERE id = %s")
```

## Strip removes them

```python
import typing

from sqlty import SQL


# sqlty: generated
class ByIdParams(typing.TypedDict):
    id: int | None


by_id = SQL[tuple[str], ByIdParams]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# stripped: by_id = "--sql\nSELECT name FROM users WHERE id = %(id)s"
```


## Classes and imports go above the first query

```python
q = "--sql\nSELECT name FROM users WHERE id = %(id)s"
import os
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class QParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], QParams]("--sql\nSELECT name FROM users WHERE id = %(id)s")
# rewritten-file: import os
```

## Strip removes a `NoParams` import from an older rewrite

```python
from sqlty import SQL
from sqlty import NoParams

q = SQL[tuple[str], NoParams]("--sql\nSELECT name FROM users")
# stripped: q = "--sql\nSELECT name FROM users"
```


## SQLite placeholders

```toml
[tool.sqlty]
dialect = "sqlite"
param-types = true
schema = "schema.sql"
```

```python
q = "--sql\nSELECT name FROM users WHERE id = ?"
r = "--sql\nSELECT name FROM users WHERE id = :id"
# rewritten-file: import typing
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file:
# rewritten-file: # sqlty: generated
# rewritten-file: class RParams(typing.TypedDict):
# rewritten-file:     id: int | None
# rewritten-file:
# rewritten-file:
# rewritten-file: q = SQL[tuple[str], tuple[int | None]]("--sql\nSELECT name FROM users WHERE id = ?")
# rewritten-file: r = SQL[tuple[str], RParams]("--sql\nSELECT name FROM users WHERE id = :id")
```

## Turned off

`param-types = false` leaves `SQL[R]`.

```toml
[tool.sqlty]
param-types = false
schema = "schema.sql"
```

```python
q = "--sql\nSELECT name FROM users WHERE id = %(id)s"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[str]]("--sql\nSELECT name FROM users WHERE id = %(id)s")
```
