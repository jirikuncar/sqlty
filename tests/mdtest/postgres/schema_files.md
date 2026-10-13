# Several schema files

`schema` takes one file or a list of files; the files are joined in order.

## One file

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

```sql
SELECT name FROM users
-- inferred: tuple[str]
```

## schema takes a list

`users.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

`orders.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, total NUMERIC(10, 2) NOT NULL);
```

```toml
[tool.sqlty]
schema = ["users.sql", "orders.sql"]
```

```sql
SELECT u.name, o.total FROM users u, orders o
-- inferred: tuple[str, float]
```
