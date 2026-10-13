# Line numbers in large files

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

## Diagnostics after many plain strings keep their line

```python
a = "one"
b = "two"
c = "three"
# error: [inline-sql-comment]
q = "--sql SELECT id FROM users"
```
