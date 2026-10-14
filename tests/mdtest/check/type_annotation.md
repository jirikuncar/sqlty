# Type Annotation Checks

## Column count mismatch — too few fields

```toml
[tool.sqlty]
dialect = "postgres"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int

# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Column count mismatch — too many fields

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

## Column type mismatch

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

## Column name mismatch

```toml
[tool.sqlty]
dialect = "postgres"
```

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

## All fields match — no error

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

## Count mismatch rule disabled — no error

```toml
[tool.sqlty]
dialect = "postgres"
[tool.sqlty.rules]
column-count-mismatch = "ignore"
```

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    id: int

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Type mismatch rule disabled — no error

```toml
[tool.sqlty]
dialect = "postgres"
[tool.sqlty.rules]
column-type-mismatch = "ignore"
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Name mismatch when count also mismatches — count reported first

Count mismatch takes priority over name/type checks.

```toml
[tool.sqlty]
dialect = "postgres"
[tool.sqlty.rules]
column-count-mismatch = "error"
column-type-mismatch = "error"
```

```python
from typing import TypedDict
from sqlty import SQL

class Row(TypedDict):
    wrong_name: str

# error: [column-count-mismatch] "Row has 1 fields but query returns 2 columns"
q = SQL[Row]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```
