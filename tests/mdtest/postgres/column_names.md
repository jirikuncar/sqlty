# PostgreSQL default column names

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

## Function and operator columns

```sql
SELECT count(*), max(id), now(), id + 1 FROM users
-- columns: count: int (BIGINT)
-- columns: max: int (INT)
-- columns: now: datetime.datetime (TIMESTAMPTZ)
-- columns: ?column?: int (INT)
```

## Columns, casts and keywords

A column keeps its name under a cast, and `lower` is the function's name.

```sql
SELECT id::BIGINT, users.name, lower(email), COALESCE(email, 'x'), CASE WHEN id > 1 THEN 1 END, EXISTS (SELECT 1), 'a', TRUE FROM users
-- columns: id: int (BIGINT)
-- columns: name: str (TEXT)
-- columns: lower: str (TEXT)
-- columns: coalesce: str (VARCHAR(255))
-- columns: case: int (None)
-- columns: exists: bool (BOOLEAN)
-- columns: ?column?: str (None)
-- columns: ?column?: bool (None)
```
