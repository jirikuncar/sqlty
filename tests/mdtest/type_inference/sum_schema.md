# SUM with a Schema

## SUM of integer column with schema returns int

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(quantity) FROM products
-- inferred: tuple[int | None]
```

## SUM of float column with schema returns float

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(price) FROM products
-- inferred: tuple[float | None]
```

## Explicit cast takes precedence over schema (1)

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(quantity::FLOAT) FROM products
-- inferred: tuple[float | None]
```

## Explicit cast takes precedence over schema (2)

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(price::INTEGER) FROM products
-- inferred: tuple[int | None]
```

## Mixing schema-inferred and cast-specified types

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(quantity), SUM(price::INTEGER) FROM products
-- inferred: tuple[int | None, int | None]
```

## Multiple SUM operations with schema inference

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(quantity), SUM(price) FROM products
-- inferred: tuple[int | None, float | None]
```

## SUM of unknown column with explicit cast works

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(unknown_col::INTEGER) FROM products
-- inferred: tuple[int | None]
```

## SUM with unknown table falls back to Decimal

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            quantity INTEGER NOT NULL,
            price FLOAT NOT NULL
        );
```

```sql
SELECT SUM(amount) FROM orders
-- inferred: tuple[decimal.Decimal | None]
```
