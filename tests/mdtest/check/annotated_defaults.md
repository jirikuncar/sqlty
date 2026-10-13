# Defaults declared through Annotated metadata

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

## TypedDict field with Field(default_factory=...)

```python
from typing import Annotated, TypedDict
from pydantic import Field
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    tags: Annotated[list[str], Field(default_factory=list)]

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## TypedDict field with Field(default=...)

```python
from typing import Annotated, TypedDict
from pydantic import Field
from sqlty import SQL

class User(TypedDict):
    id: int
    email: Annotated[str, Field(default="")]

# error:
query = SQL[User]("--sql\nSELECT id FROM users")
```

## Required and NotRequired wrappers around Annotated

```python
from typing import Annotated, NotRequired, TypedDict
from pydantic import Field
from sqlty import SQL

class User(TypedDict):
    id: int
    tags: NotRequired[Annotated[list[str], Field(default_factory=list)]]

# error:
query = SQL[User]("--sql\nSELECT id FROM users")
```

## A bare Field() keeps the field required

```python
from typing import Annotated, TypedDict
from pydantic import Field
from sqlty import SQL

class User(TypedDict):
    id: int
    name: Annotated[str, Field(...)]
    email: Annotated[str, Field()]

# error: [column-count-mismatch] "User has 3 fields but query returns 1 columns"
query = SQL[User]("--sql\nSELECT id FROM users")
```

## Dataclass and Pydantic fields with Annotated defaults

```python
from dataclasses import dataclass, field
from typing import Annotated
from pydantic import BaseModel, Field
from sqlty import SQL

@dataclass
class Account:
    id: int
    tags: Annotated[list[str], field(default_factory=list)]

class Profile(BaseModel):
    id: int
    tags: Annotated[list[str], Field(default_factory=list)]

# error:
a = SQL[Account]("--sql\nSELECT id FROM users")
# error:
b = SQL[Profile]("--sql\nSELECT id FROM users")
```

## An optional field is still type checked

```python
from typing import Annotated, TypedDict
from pydantic import Field
from sqlty import SQL

class User(TypedDict):
    id: int
    name: Annotated[int, Field(default=0)]

# error: [column-type-mismatch] "User.name is Annotated[int, Field(default=0)] but query column 'name' infers str"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```
