# Schema-Based Inference

Use a `CREATE TABLE` schema file so SQLTy can infer types without explicit `::TYPE` casts.

## Setup

Provide a schema file with your table definitions:

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

## Write queries (no casts needed)

```python
user_query = "--sql\nSELECT id, name, email FROM users"
```

## Rewrite with schema

This example's `pyproject.toml` sets `schema = "schema.sql"` under
`[tool.sqlty]`, so no flag is needed:

```bash
sqlty rewrite .
```

`--schema other.sql` overrides the configured schema.

Result:

```python
from sqlty import SQL

user_query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

Types are inferred from the schema: `id` is `INTEGER` -> `int`, `name` is `TEXT` -> `str`, etc.

## Star expansion

With a schema loaded, `SELECT *` is expanded to explicit columns:

```bash
sqlty rewrite .
```

`SELECT * FROM users` becomes `SELECT id::INTEGER, name::TEXT, email::VARCHAR, created_at::TIMESTAMP FROM users`.
