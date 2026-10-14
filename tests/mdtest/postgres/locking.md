# PostgreSQL row-lock strengths

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

## FOR NO KEY UPDATE

```sql
SELECT id, name FROM users WHERE id = %(id)s FOR NO KEY UPDATE
-- inferred: tuple[int, str]
```

## FOR KEY SHARE

```sql
SELECT id FROM users WHERE id = %(id)s FOR KEY SHARE
-- inferred: tuple[int]
```

## FOR UPDATE OF several tables

```sql
SELECT u.id, o.status FROM users u JOIN orders o ON o.user_id = u.id FOR UPDATE OF u, o
-- inferred: tuple[int, str | None]
```

## Empty target list locks rows without returning columns

```sql
SELECT FROM users WHERE id = %(id)s FOR UPDATE
-- inferred: tuple[()]
```
