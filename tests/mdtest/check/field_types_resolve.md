# Field types resolve one by one

A row type field whose annotation cannot be resolved is unknown and never
reported. Resolving one field does not depend on the others, nor on every
import of the file being available.

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

## from __future__ import annotations

```python
from __future__ import annotations

from datetime import datetime
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    created_at: datetime

# error:
query = SQL[User]("--sql\nSELECT id, created_at FROM users")
```

## An import that cannot be resolved

```python
from datetime import datetime
from typing import TypedDict
from myapp.ids import UserId
from sqlty import SQL

class User(TypedDict):
    id: UserId
    created_at: datetime

# error:
query = SQL[User]("--sql\nSELECT id, created_at FROM users")
```

## A resolved field is still checked next to an unresolved one

```python
from typing import TypedDict
from myapp.ids import UserId
from sqlty import SQL

class User(TypedDict):
    id: UserId
    name: int

# error: [column-type-mismatch] "User.name is int but query column 'name' infers str"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## A type alias from the same module

```python
from typing import TypedDict
from sqlty import SQL

UserName = str

class User(TypedDict):
    id: int
    name: UserName | None

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## A `type` statement from the same module

```python
from typing import TypedDict
from sqlty import SQL

type UserName = str

class User(TypedDict):
    id: int
    name: UserName

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## A dataclass with an unresolved import

```python
from dataclasses import dataclass
from datetime import datetime
from myapp.ids import UserId
from sqlty import SQL

@dataclass
class User:
    id: UserId
    created_at: datetime

# error:
query = SQL[User]("--sql\nSELECT id, created_at FROM users")
```
