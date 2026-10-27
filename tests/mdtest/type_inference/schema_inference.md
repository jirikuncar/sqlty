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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id, name, email FROM users
-- inferred: tuple[int, str, str | None]
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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id::FLOAT, name FROM users
-- inferred: tuple[float, str]
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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT SUM(quantity) FROM products
-- inferred: tuple[int | None]
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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT SUM(price) FROM products
-- inferred: tuple[float | None]
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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT SUM(quantity::FLOAT) FROM products
-- inferred: tuple[float | None]
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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT users.id, users.name, orders.total, orders.created_at
FROM users
JOIN orders ON users.id = orders.user_id
-- inferred: tuple[int, str, float, datetime.datetime | None]
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
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT id, name, active, birth_date FROM users
-- inferred: tuple[int, str, bool | None, datetime.date | None]
```

## No schema — columns without casts are unknown

```toml
[tool.sqlty]
dialect = "postgres"
```

```sql
SELECT id, name FROM users
-- inferred: None
```

## SELECT * follows the schema's column order

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT, score NUMERIC(10, 2));
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM users
-- inferred: tuple[int | None, str | None, decimal.Decimal | None]
```

## Qualified star and aliased columns resolve through table aliases

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER, placed TIMESTAMP);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT u.*, o.placed FROM users u JOIN orders o ON o.user_id = u.id
-- inferred: tuple[int, str | None, datetime.datetime | None]
```

## Unqualified columns search the joined tables in order

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER, placed TIMESTAMP);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT name, placed FROM users JOIN orders ON user_id = users.id
-- inferred: tuple[str | None, datetime.datetime | None]
```

## Star over a USING or NATURAL join lists each merged column once

A merged column is never NULL in an inner join, since NULL keys don't match. In an
outer join it has the value of the preserved side, and of either side in a `FULL JOIN`.
PostgreSQL lists the merged columns first, in the order of the `USING` list.

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT);
CREATE TABLE orders (total INTEGER, id INTEGER, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM users JOIN orders USING (name, id)
-- columns: name: str (TEXT)
-- columns: id: int (INT)
-- columns: total: int | None (INT)
```

```sql
SELECT * FROM users NATURAL RIGHT JOIN orders
-- columns: id: int | None (INT)
-- columns: name: str (TEXT)
-- columns: total: int | None (INT)
```

```sql
SELECT * FROM users FULL JOIN orders USING (id)
-- columns: id: int | None (INT)
-- columns: name: str | None (TEXT)
-- columns: total: int | None (INT)
-- columns: name: str | None (TEXT)
```

### MySQL

MySQL lists the merged columns first, in the order of the first table, and reads a
`RIGHT JOIN` as a `LEFT JOIN` with the tables swapped.

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT);
CREATE TABLE orders (total INTEGER, id INTEGER, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "mysql"
```

```sql
SELECT * FROM users JOIN orders USING (name, id)
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: total: int | None (INT)
```

```sql
SELECT * FROM users RIGHT JOIN orders USING (id)
-- columns: id: int | None (INT)
-- columns: total: int | None (INT)
-- columns: name: str (TEXT)
-- columns: name: str | None (TEXT)
```

### SQLite

SQLite keeps the left table's columns in place and drops the right table's copies.

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT);
CREATE TABLE orders (total INTEGER, id INTEGER, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "sqlite"
```

```sql
SELECT * FROM users JOIN orders USING (name, id)
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: total: int | None (INT)
```

```sql
SELECT * FROM users LEFT JOIN orders USING (id)
-- columns: id: int (INT)
-- columns: name: str | None (TEXT)
-- columns: total: int | None (INT)
-- columns: name: str | None (TEXT)
```

## Star over a table missing from the schema has no row type

The columns of `*` are unknown, so no type is inferred and nothing is guessed.

`schema.sql`:
```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM audit_log
-- inferred: None
```
