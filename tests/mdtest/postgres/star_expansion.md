# Star Expansion

## Single table — all columns expanded

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users
-- expanded: SELECT id::INT, name::TEXT, email::VARCHAR(255) FROM users
```

## Qualified star — table prefix

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT users.* FROM users
-- expanded: SELECT id::INT, name::TEXT FROM users
```

## No schema — returns None

```sql
SELECT * FROM users
-- expanded: None
```

## No star — returns None

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
-- expanded: None
```

## Table alias with qualified star

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT u.* FROM users u
-- expanded: SELECT id::INT, name::TEXT FROM users u
```

## Multi-table star expansion

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE orders (
    order_id INTEGER PRIMARY KEY,
    amount NUMERIC NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users, orders
-- expanded: SELECT id::INT, name::TEXT, order_id::INT, amount::DECIMAL FROM users, orders
```

## Unknown table — returns None

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM unknown_table
-- expanded: None
```

## Other projections are kept verbatim

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT *, id::INTEGER + 1 AS next_id FROM users
-- expanded: SELECT id::INT, name::TEXT, id::INTEGER + 1 AS next_id FROM users
```
