# Inline suppression

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);
```
```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Ignore one rule on one line

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error:
query = SQL[User]("--sql\nSELECT id FROM users")  # sqlty: ignore[column-type-mismatch]
```

## Bare ignore drops every diagnostic on its line

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")  # sqlty: ignore
```

## Several rules in one comment

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: str
    email: str

# error:
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")  # sqlty: ignore[column-type-mismatch, typeddict-key-mismatch]
```

## Unlisted rule is still reported

The comment names a rule that does not fire on its line, so the type mismatch stays.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[column-name-mismatch]
```

## Ignore only covers its own line

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")

# error:
r = SQL[User]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[column-type-mismatch]
```

## A hash inside a string is not a comment

The comment text sits in a string on the reported line, so the diagnostic stays.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users WHERE name = '# sqlty: ignore'")
```

## Ignore in a multi-line call applies to the reported line

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error:
q = SQL[User](  # sqlty: ignore[column-type-mismatch]
    "--sql\nSELECT id::INTEGER FROM users"
)
```

## Fix keeps the comment

`check --fix` rewrites the query and leaves the suppression in place.

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")  # sqlty: ignore[inline-sql-comment]
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")  # sqlty: ignore[inline-sql-comment]
```
