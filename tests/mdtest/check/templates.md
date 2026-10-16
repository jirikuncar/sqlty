# t-string queries

`SQLTemplate[T](t"...")` is checked like `SQL[T]("...")`; each field is a
parameter.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

## stale-type and row types

```python
from typing import TypedDict
from sqlty import SQLTemplate

class User(TypedDict):
    id: str
    name: str

user_id = 1
# error: [stale-type] "SQLTemplate[tuple[str]] should be SQLTemplate[tuple[int]]"
a = SQLTemplate[tuple[str]](t"--sql\nSELECT id FROM users WHERE id = {user_id}")
# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
b = SQLTemplate[User](t"--sql\nSELECT id, name FROM users WHERE id = {user_id}")
c = SQLTemplate[tuple[int, str]](t"--sql\nSELECT id, name FROM users WHERE id = {user_id}")
```

## check --fix

```python
from sqlty import SQLTemplate

user_id = 1
q = SQLTemplate[tuple[str]](t"--sql\nSELECT id FROM users WHERE id = {user_id}")
# fixed: from sqlty import SQLTemplate
# fixed:
# fixed: user_id = 1
# fixed: q = SQLTemplate[tuple[int]](t"--sql\nSELECT id FROM users WHERE id = {user_id}")
```
