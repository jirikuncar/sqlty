# Parameter types

With `param-types = true`, `rewrite` writes the type of the parameters of each query,
`SQL[R, P]`: a `tuple` for positional parameters, and a generated `TypedDict` for named ones.

```toml
[tool.sqlty]
param-types = true
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

```python
q = "--sql\nSELECT name FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[str], tuple[()]]("--sql\nSELECT name FROM users")
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
