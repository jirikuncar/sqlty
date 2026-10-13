# User-Defined Type Checking

Validates that SQL[UserType](...) annotations match the query columns
structurally, supporting TypedDict, NamedTuple, dataclass, and Pydantic
BaseModel types.

## TypedDict — all fields match

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## TypedDict — field count mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

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

## TypedDict — field type mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## TypedDict — field name mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class UserRow(TypedDict):
    user_id: int
    username: str

# error: [typeddict-key-mismatch] "UserRow has no key for query column 'id'"
# error: [typeddict-key-mismatch] "UserRow has no key for query column 'name'"
# error: [typeddict-key-mismatch] "UserRow.user_id is required but missing from query"
# error: [typeddict-key-mismatch] "UserRow.username is required but missing from query"
q = SQL[UserRow]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## NamedTuple — all fields match

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import NamedTuple
from sqlty import SQL

class UserRow(NamedTuple):
    id: int
    name: str

q = SQL[UserRow]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## NamedTuple — type mismatch with datetime

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import NamedTuple
import datetime
from sqlty import SQL

class EventRow(NamedTuple):
    id: int
    created_at: str

# error: [column-type-mismatch] "EventRow.created_at is str but query column 'created_at' infers datetime.datetime"
q = SQL[EventRow]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
```

## dataclass — all fields match

```toml
[tool.sqlty]
dialect = "postgres"
```

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

## dataclass — count mismatch (too few fields)

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from dataclasses import dataclass
from sqlty import SQL

@dataclass
class Product:
    id: int

# error: [column-count-mismatch] "Product has 1 fields but query returns 3 columns"
q = SQL[Product]("--sql\nSELECT id::INTEGER, name::TEXT, price::NUMERIC FROM products")
```

## Pydantic BaseModel — all fields match

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from pydantic import BaseModel
from sqlty import SQL

class UserModel(BaseModel):
    id: int
    name: str
    email: str

q = SQL[UserModel]("--sql\nSELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")
# error:
```

## Pydantic BaseModel — type mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from pydantic import BaseModel
from sqlty import SQL

class UserModel(BaseModel):
    id: int
    name: int

# error: [column-type-mismatch] "UserModel.name is int but query column 'name' infers str"
q = SQL[UserModel]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Single field type — uuid mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
import uuid
from sqlty import SQL

class TokenRow(TypedDict):
    token_id: str

# error: [column-type-mismatch] "TokenRow.token_id is str but query column 'token_id' infers uuid.UUID"
q = SQL[TokenRow]("--sql\nSELECT token_id::UUID FROM tokens")
```

## Multiple mismatches — count takes priority

When both count and type mismatches exist, count is reported first.

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class WrongType(TypedDict):
    bad_name: str

# error: [column-count-mismatch] "WrongType has 1 fields but query returns 2 columns"
q = SQL[WrongType]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Schema-based inference with user type

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str
    email: str

q = SQL[User]("--sql\nSELECT id, name, email FROM users")
# error:
```

## Schema-based — type mismatch without explicit casts

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str
    email: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id, name, email FROM users")
```
