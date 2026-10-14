# Annotated helpers defined as methods

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Static and instance methods

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def fetch_all[T](self, query: Annotated[LiteralString, SQL], form: type[T], /) -> list[T]: ...

    @staticmethod
    def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T | None: ...

db = Db()
# error: [column-type-mismatch]
ids = db.fetch_all("SELECT name FROM users", int)
# error: [column-type-mismatch]
one = Db.fetch_one("SELECT name FROM users", int)
```
