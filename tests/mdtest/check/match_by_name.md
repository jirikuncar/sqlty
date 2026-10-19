# Match row fields by name

TypedDict, dataclass and Pydantic fields are matched to columns by name, as
`dict_row`, `class_row` and `kwargs_row` pass them. `NamedTuple` fields are
matched by position.

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## TypedDict keys in a different order

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    name: str
    id: int

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## TypedDict types are checked by name

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    name: int
    id: int

# error: [column-type-mismatch] "User.name is int but query column 'name' infers str"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## TypedDict column without a key

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# error: [column-count-mismatch] "User has 2 fields but query returns 3 columns"
query = SQL[User]("--sql\nSELECT id, name, email FROM users")
```

## TypedDict column that matches no key

```python
from typing import NotRequired, TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: NotRequired[str]

# error: [typeddict-key-mismatch] "User has no key for query column 'created_at'"
query = SQL[User]("--sql\nSELECT id, name, created_at FROM users")
```

## Dataclass fields in a different order

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    name: str
    id: int

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Dataclass fields with defaults are optional

```python
from dataclasses import dataclass, field
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str
    email: str | None = None
    tags: list[str] = field(default_factory=list)

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Dataclass required field missing from the query

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str
    email: str | None = None

# error: [column-count-mismatch] "User has 2 required and 3 total fields but query returns 1 columns"
query = SQL[User]("--sql\nSELECT id FROM users")
```

## Dataclass column without a field

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str

# error: [column-count-mismatch] "User has 2 fields but query returns 3 columns"
query = SQL[User]("--sql\nSELECT id, name, email FROM users")
```

## Dataclass column that matches no field

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class User:
    id: int
    name: str
    email: str | None = None

# error: [typeddict-key-mismatch] "User has no field for query column 'created_at'"
query = SQL[User]("--sql\nSELECT id, name, created_at FROM users")
```

## Dataclass annotations that take no column

A `ClassVar`, the `KW_ONLY` marker and a `field(init=False)` are not arguments
of the constructor `class_row` calls, so no column fills them.

```python
from dataclasses import KW_ONLY, dataclass, field
from typing import ClassVar
from sqlty import SQL

@dataclass
class User:
    id: int
    table: ClassVar[str]
    _: KW_ONLY
    name: str
    label: str = field(init=False, default="")

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
# error: [column-count-mismatch] "User has 2 fields but query returns 3 columns"
labelled = SQL[User]("--sql\nSELECT id, name, name AS label FROM users")
```

## Pydantic fields with defaults are optional

```python
from pydantic import BaseModel
from sqlty import SQL

class User(BaseModel):
    id: int
    name: str
    email: str | None = None

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Pydantic reports columns without a field

Pydantic drops them by default (`extra="ignore"`), so they are fetched for nothing.

```python
from pydantic import BaseModel
from sqlty import SQL

class User(BaseModel):
    id: int
    name: str

# error: [typeddict-key-mismatch] "User has no field for query column 'created_at'"
# error: [typeddict-key-mismatch] "User has no field for query column 'email'"
query = SQL[User]("--sql\nSELECT id, name, email, created_at FROM users")
```

## Pydantic class variables and private attributes

A `ClassVar` and a private `_attribute` are not fields of a model.

```python
from typing import ClassVar
from pydantic import BaseModel
from sqlty import SQL

class User(BaseModel):
    id: int
    table: ClassVar[str]
    _cache: dict[str, int] = {}

# error:
query = SQL[User]("--sql\nSELECT id FROM users")
# error: [typeddict-key-mismatch] "User has no field for query column '_cache'"
cached = SQL[User]("--sql\nSELECT id, name AS _cache FROM users")
```

## Pydantic required field missing from the query

```python
from pydantic import BaseModel, Field
from sqlty import SQL

class User(BaseModel):
    id: int
    name: str = Field(...)

# error: [column-count-mismatch] "User has 2 required fields but query returns 1 columns"
query = SQL[User]("--sql\nSELECT id FROM users")
```

## NamedTuple stays positional

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    name: str
    id: int

# error: [column-name-mismatch] "User.id doesn't match query column 'name'"
# error: [column-name-mismatch] "User.name doesn't match query column 'id'"
# error: [column-type-mismatch] "User.id is int but query column 'name' infers str"
# error: [column-type-mismatch] "User.name is str but query column 'id' infers int"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```
