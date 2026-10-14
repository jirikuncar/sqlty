# Nullability: lint checks against row types

`nullable-mismatch` reports a row field that is not optional for a column that can be
NULL; `redundant-optional` (opt-in) reports an optional field for a column that cannot be.

## A non-optional field for a nullable column

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, nick TEXT NOT NULL);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    nick: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name, nick FROM users")
```

## Optional fields are accepted

`T | None` and `Optional[T]` fit a nullable column, and also a column that cannot be
NULL: a wider annotation is not an error, only `redundant-optional` reports it.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, nick TEXT NOT NULL);
```

```python
from typing import Optional, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int | None
    name: Optional[str]
    nick: str | None

q = SQL[User]("--sql\nSELECT id, name, nick FROM users")
# error:
```

## Dataclasses, NamedTuples and Pydantic models

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

```python
from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    id: int
    name: str

# error: [nullable-mismatch] "Row.name is str but query column 'name' can be NULL"
q = SQL[Row]("--sql\nSELECT id, name FROM users")
```

```python
from pydantic import BaseModel
from sqlty import SQL

class Model(BaseModel):
    id: int
    name: str

# error: [nullable-mismatch] "Model.name is str but query column 'name' can be NULL"
q = SQL[Model]("--sql\nSELECT id, name FROM users")
```

## Outer joins

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, title TEXT NOT NULL);
```

```python
from typing import TypedDict
from sqlty import SQL

class UserPost(TypedDict):
    name: str
    title: str

# error: [nullable-mismatch] "UserPost.title is str but query column 'title' can be NULL"
q = SQL[UserPost]("--sql\nSELECT u.name, p.title FROM users u LEFT JOIN posts p ON p.user_id = u.id")
```

## Unknown nullability is never reported

Casts with no schema carry no nullability, so they are neither optional nor reported.

```toml
[tool.sqlty]

[tool.sqlty.rules]
redundant-optional = "error"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str | None

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Fields typed Any are never reported

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import Any, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: Any
    name: Any

q = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
```

## redundant-optional

Strict mode: enable the rule to flag `| None` on columns that cannot be NULL. It stays
off unless enabled.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
redundant-optional = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import Optional, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: Optional[int]
    name: str | None

# error: [redundant-optional] "User.id is int | None but query column 'id' is never NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Switching nullable-mismatch off

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
nullable-mismatch = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
```

## Lowering the severity of nullable-mismatch

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
nullable-mismatch = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# error: [nullable-mismatch] "User.name is str but query column 'name' can be NULL"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Helper row types

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```python
from typing import Annotated, LiteralString

from sqlty import SQL


def fetch[T](query: Annotated[LiteralString, SQL], row: type[T]) -> list[T]:
    raise NotImplementedError


# error: [nullable-mismatch] "tuple[int, str] item 1 is str but query column 'name' can be NULL"
rows = fetch("SELECT id, name FROM users", tuple[int, str])
rows = fetch("SELECT id, name FROM users", tuple[int, str | None])
```
