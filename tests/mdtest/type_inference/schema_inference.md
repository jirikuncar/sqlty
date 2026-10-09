# Schema-Aware Type Inference

When a schema file is loaded, column types are inferred from
CREATE TABLE definitions instead of requiring explicit casts.

## Basic schema lookup

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
SELECT id, name, email FROM users
```

```python
# inferred: tuple[int, str, str]
```

## Schema with explicit cast override

Explicit casts override schema-inferred types:

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
SELECT id::FLOAT, name FROM users
```

```python
# inferred: tuple[float, str]
```

## SUM infers type from schema

`schema.sql`:
```sql
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    quantity INTEGER NOT NULL,
    price FLOAT NOT NULL
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT SUM(quantity) FROM products
```

```python
# inferred: tuple[int]
```

## SUM of float column from schema

`schema.sql`:
```sql
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    quantity INTEGER NOT NULL,
    price FLOAT NOT NULL
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT SUM(price) FROM products
```

```python
# inferred: tuple[float]
```

## SUM with explicit cast overrides schema

`schema.sql`:
```sql
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    quantity INTEGER NOT NULL,
    price FLOAT NOT NULL
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT SUM(quantity::FLOAT) FROM products
```

```python
# inferred: tuple[float]
```

## Multiple tables in schema — full table names

Schema lookup uses full table names. Table aliases (`u.`, `o.`) don't resolve
against schema (known limitation — alias resolution is not implemented yet).

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total FLOAT NOT NULL,
    created_at TIMESTAMP
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT users.id, users.name, orders.total, orders.created_at
FROM users
JOIN orders ON users.id = orders.user_id
```

```python
# inferred: tuple[int, str, float, datetime.datetime]
```

## Schema with boolean and date types

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    active BOOLEAN DEFAULT true,
    birth_date DATE
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id, name, active, birth_date FROM users
```

```python
# inferred: tuple[int, str, bool, datetime.date]
```

## No schema — columns without casts default to str

```toml
[sqlty]
dialect = "postgres"
```

```sql
SELECT id, name FROM users
```

```python
# inferred: tuple[str, str]
```

## SELECT * follows the schema's column order

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10, 2));
```

```toml
[sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM users
```

```python
# inferred: tuple[int, str, float]
```

## Qualified star and aliased columns resolve through table aliases

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER, placed TIMESTAMP);
```

```toml
[sqlty]
schema = "schema.sql"
```

```sql
SELECT u.*, o.placed FROM users u JOIN orders o ON o.user_id = u.id
```

```python
# inferred: tuple[int, str, datetime.datetime]
```

## Unqualified columns search the joined tables in order

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER, placed TIMESTAMP);
```

```toml
[sqlty]
schema = "schema.sql"
```

```sql
SELECT name, placed FROM users JOIN orders ON user_id = users.id
```

```python
# inferred: tuple[str, datetime.datetime]
```

## Star over a table missing from the schema stays one column

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```toml
[sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM audit_log
```

```python
# inferred: tuple[str]
```
