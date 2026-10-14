# Helper calls in nested scopes

Bodies that never mention a helper are not searched for calls; the others are,
at any depth.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(255));
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Nested function in a method

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

class Repo:
    def names(self):
        def inner():
            # error: [column-type-mismatch]
            return fetch_all("SELECT name FROM users", int)
        return inner()

def unrelated(x):
    return [a for a in range(x)]
```
