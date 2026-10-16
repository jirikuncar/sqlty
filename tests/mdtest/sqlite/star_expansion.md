# SQLite star expansion

SQLite has no `::` cast, so `check --fix` expands `*` to the bare column names;
the schema types them.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## Expansion

```sql
SELECT * FROM users
-- expanded: SELECT id, name FROM users
```

```sql
SELECT u.* FROM users u
-- expanded: SELECT u.id, u.name FROM users u
```

## check --fix

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users")
```
