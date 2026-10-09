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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users
```

```python
# expanded: SELECT id::INT, name::TEXT, email::VARCHAR(255) FROM users
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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT users.* FROM users
```

```python
# expanded: SELECT id::INT, name::TEXT FROM users
```

## No schema — returns None

```sql
SELECT * FROM users
```

```python
# expanded: None
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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# expanded: None
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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT u.* FROM users u
```

```python
# expanded: SELECT id::INT, name::TEXT FROM users u
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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users, orders
```

```python
# expanded: SELECT id::INT, name::TEXT, order_id::INT, amount::DECIMAL FROM users, orders
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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM unknown_table
```

```python
# expanded: None
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
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT *, id::INTEGER + 1 AS next_id FROM users
```

```python
# expanded: SELECT id::INT, name::TEXT, id::INTEGER + 1 AS next_id FROM users
```
