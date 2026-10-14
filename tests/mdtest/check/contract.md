# Check Contract

## `check_source`

### Detects stale rewrite space

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT 1::INTEGER"
```

### Detects stale rewrite newline

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT 1::INTEGER"
```

### Clean source no stale rewrite

```python
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# error:
```

### Detects stale type

```python
# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM t")
```

### Correct type no stale

```python
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM t")
# error:
```

### Rule override ignore

```toml
[tool.sqlty]
[tool.sqlty.rules]
stale-rewrite = "ignore"
```

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT 1::INTEGER"
```

### Returns list

```python
x = 1
# error:
```

### Non sql source returns empty

```python
x = 1
y = 2
# error:
```

### Detects --sql<space> even inside SQL[T](...)

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
```

### Rule can be disabled via overrides

```toml
[tool.sqlty]
[tool.sqlty.rules]
inline-sql-comment = "ignore"
```

```python
# error:
q = "--sql SELECT 1::INTEGER"
```

## Untyped sentinel

### Check untyped now inferable

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL);
```

```python
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; run sqlty check --fix"
q = SQL[UNTYPED]("--sql\nSELECT id FROM users")
```

## User types

### TypedDict with matching fields produces no diagnostics

```python

from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

### TypedDict with wrong field count is detected

```python

from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

# error: [column-count-mismatch] "User has 3 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### TypedDict with wrong field type is detected

```python

from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### NamedTuple with wrong field name is detected

```python

from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    user_id: int
    username: str

# error: [column-name-mismatch] "User.user_id doesn't match query column 'id'"
# error: [column-name-mismatch] "User.username doesn't match query column 'name'"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### NamedTuple with matching fields produces no diagnostics

```python

from typing import NamedTuple
from sqlty import SQL

class UserRow(NamedTuple):
    id: int
    name: str

q = SQL[UserRow]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

### NamedTuple field type mismatch is detected

```python

from typing import NamedTuple
from sqlty import SQL

class Row(NamedTuple):
    id: int
    name: int

# error: [column-type-mismatch] "Row.name is int but query column 'name' infers str"
q = SQL[Row]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Dataclass with matching fields produces no diagnostics

```python

from dataclasses import dataclass
from decimal import Decimal
from sqlty import SQL

@dataclass
class Product:
    id: int
    name: str
    price: Decimal

q = SQL[Product]("--sql\nSELECT id::INTEGER, name::TEXT, price::NUMERIC FROM products")
# error:
```

### Dataclass field count mismatch is detected

```python

from dataclasses import dataclass
from sqlty import SQL

@dataclass
class Product:
    id: int

# error: [column-count-mismatch] "Product has 1 fields but query returns 2 columns"
q = SQL[Product]("--sql\nSELECT id::INTEGER, name::TEXT FROM products")
```

### Pydantic BaseModel with matching fields produces no diagnostics

```python

from pydantic import BaseModel
from sqlty import SQL

class UserModel(BaseModel):
    id: int
    name: str

q = SQL[UserModel]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

### Pydantic BaseModel field type mismatch is detected

```python

from pydantic import BaseModel
from sqlty import SQL

class UserModel(BaseModel):
    id: int
    name: int

# error: [column-type-mismatch] "UserModel.name is int but query column 'name' infers str"
q = SQL[UserModel]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### SQL[UnknownType] should NOT trigger stale-type (can't validate)

```python
q = SQL[SomeExternalType]("--sql\nSELECT id::INTEGER FROM t")
# error:
```

### Optional[int] / int | None should match int

```python

from typing import TypedDict

from sqlty import SQL

class User(TypedDict):
    id: int | None
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

### Datetime.datetime field matches TIMESTAMP column

```python

import datetime
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    created_at: datetime.datetime

q = SQL[Event]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
# error:
```

### Uuid.UUID field matches UUID column

```python

import uuid
from typing import TypedDict
from sqlty import SQL

class Token(TypedDict):
    token_id: uuid.UUID

q = SQL[Token]("--sql\nSELECT token_id::UUID FROM tokens")
# error:
```

### 'from datetime import datetime' matches TIMESTAMP column

```python

from datetime import datetime
from typing import TypedDict
from sqlty import SQL

class Event(TypedDict):
    id: int
    created_at: datetime

q = SQL[Event]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
# error:
```

### 'from datetime import date' matches DATE column

```python

from datetime import date
from typing import TypedDict
from sqlty import SQL

class Record(TypedDict):
    event_date: date

q = SQL[Record]("--sql\nSELECT event_date::DATE FROM records")
# error:
```

### 'from uuid import UUID' matches UUID column

```python

from uuid import UUID
from typing import TypedDict
from sqlty import SQL

class Token(TypedDict):
    token_id: UUID

q = SQL[Token]("--sql\nSELECT token_id::UUID FROM tokens")
# error:
```

## Regression cases

### Check skips user type annotations

```python
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

### Check select star message

```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nselect * FROM users")
```

### `CREATE TABLE users (id INT NOT NULL);`

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT NOT NULL);
```

```python
# error: [select-star] "SELECT * used — expand to explicit column list (joins need a manual edit)"
# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nselect * FROM users")
```

## Check_source detects unwrapped --sql strings

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT id::INTEGER FROM users"
```

## Check_source returns empty for clean wrapped source

```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# error:
```

## Check_source detects stale type annotations

```python
# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[int]]"
q = SQL[tuple[str]]("--sql\nSELECT id::INTEGER FROM users")
```

## Check_source respects rule severity overrides

```toml
[tool.sqlty]
[tool.sqlty.rules]
stale-rewrite = "ignore"
```

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT id::INTEGER FROM users"
```

## Closed TypedDict key-set violations are errors unless reconfigured (1)

```python
from typing import TypedDict
from sqlty import SQL
class User(TypedDict, closed=True):
    id: int
    name: str
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Closed TypedDict key-set violations are errors unless reconfigured (2)

```toml
[tool.sqlty]
[tool.sqlty.rules]
typeddict-key-mismatch = "warn"
```

```python
from typing import TypedDict
from sqlty import SQL
class User(TypedDict, closed=True):
    id: int
    name: str
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Closed TypedDict key-set violations are errors unless reconfigured (3)

```toml
[tool.sqlty]
[tool.sqlty.rules]
column-name-mismatch = "error"
```

```python
from typing import TypedDict
from sqlty import SQL
class User(TypedDict, closed=True):
    id: int
    name: str
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
```

## Check_source respects inline-sql-comment = ignore

```toml
[tool.sqlty]
[tool.sqlty.rules]
inline-sql-comment = "ignore"
```

```python
# error:
q = "--sql SELECT id::INTEGER FROM users"
```
