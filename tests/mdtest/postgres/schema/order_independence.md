# Any file order

A statement whose dependencies are not defined yet is retried, so a schema that
lists migrations out of order (or a directory of files read alphabetically) loads
the same as one in dependency order.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## ALTER before CREATE

`schema.sql`:

```sql
ALTER TABLE users ADD COLUMN age INTEGER;
ALTER TABLE users RENAME COLUMN age TO years;
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
```

```sql
SELECT * FROM users
-- inferred: tuple[int, str, int]
```

## A rename and an add that need the other

`schema.sql`:

```sql
ALTER TABLE users RENAME COLUMN age TO years;
ALTER TABLE users ADD COLUMN age INTEGER;
CREATE TABLE users (id INTEGER);
```

```sql
SELECT * FROM users
-- inferred: tuple[int, int]
```

## Views before their tables and before each other

`schema.sql`:

```sql
CREATE VIEW adults AS SELECT id, name FROM visitors WHERE age >= 18;
CREATE VIEW visitors AS SELECT id, name, age FROM people;
CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, age INTEGER);
```

```sql
SELECT id, name FROM adults
-- inferred: tuple[int, str]
```

## Types after the tables that use them

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER, status order_status, discount percent);
ALTER TYPE order_status ADD VALUE 'refunded';
CREATE DOMAIN percent AS integer;
CREATE TYPE order_status AS ENUM ('new', 'paid');
```

```sql
SELECT * FROM orders
-- inferred: tuple[int, typing.Literal['new', 'paid', 'refunded'], int]
```

## A view over something the schema never defines

The view is still analyzed; what it cannot resolve stays unknown.

`schema.sql`:

```sql
CREATE VIEW v AS SELECT id, 1 AS one FROM missing;
```

```sql
SELECT one FROM v
-- inferred: tuple[int]
```

## Functions after their callers

`schema.sql`:

```sql
CREATE VIEW v AS SELECT answer() AS a;
CREATE FUNCTION answer() RETURNS integer LANGUAGE sql AS 'SELECT 42';
```

```sql
SELECT a FROM v
-- inferred: tuple[int]
```
