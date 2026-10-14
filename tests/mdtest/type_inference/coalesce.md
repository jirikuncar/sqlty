# COALESCE ignores NULL and placeholder arguments

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

## Placeholder first

```sql
SELECT coalesce(%(since)s, created_at) AS since FROM users
-- inferred: tuple[datetime.datetime | None]
```

## NULL first

```sql
SELECT coalesce(NULL, created_at) AS created_at FROM users
-- inferred: tuple[datetime.datetime | None]
```

## GREATEST and LEAST skip them too

```sql
SELECT greatest(%(a)s, NULL, id) AS g, least(NULL, name) AS l FROM users
-- columns: g: int (INT)
-- columns: l: str (TEXT)
```

## Nothing known stays unknown

```sql
SELECT coalesce(NULL, %(x)s) AS x
-- columns: x: None (None)
```
