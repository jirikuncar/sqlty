# Nullability of expressions (MySQL and SQLite)

The rules that do not depend on PostgreSQL syntax hold in every dialect; the ones
that differ follow each database.

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INT NOT NULL PRIMARY KEY,
    name VARCHAR(50) NOT NULL,
    nick VARCHAR(50),
    age INT
);
```

`IFNULL` and `COALESCE` take the first non-null argument.

```sql
SELECT CAST(ifnull(nick, name) AS CHAR), CAST(ifnull(nick, nick) AS CHAR), coalesce(age, 0) FROM users
-- inferred: tuple[str, str | None, int]
```

`CONCAT`, `GREATEST` and `LEAST` are NULL when any argument is, unlike in PostgreSQL.

```sql
SELECT CAST(concat(name, nick) AS CHAR), CAST(concat(name, name) AS CHAR), CAST(greatest(age, 1) AS SIGNED) FROM users
-- inferred: tuple[str | None, str, int | None]
```

`IF` is NULL when a branch can be.

```sql
SELECT CAST(if(age > 18, name, nick) AS CHAR), CAST(if(age > 18, name, 'minor') AS CHAR) FROM users
-- inferred: tuple[str | None, str]
```

Aggregates follow the same rules as in PostgreSQL.

```sql
SELECT count(*), max(id), max(age) FROM users
-- inferred: tuple[int, int | None, int | None]
```

```sql
SELECT name, max(id), max(age) FROM users GROUP BY name
-- inferred: tuple[str, int, int | None]
```

## SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    nick TEXT,
    age INTEGER
);
```

```sql
SELECT CAST(ifnull(nick, name) AS TEXT), coalesce(nick, name), CAST(iif(age > 18, name, nick) AS TEXT) FROM users
-- inferred: tuple[str, str, str | None]
```

The two-argument `MIN` and `MAX` are scalar functions, NULL when an argument is; with one
argument they aggregate.

```sql
SELECT max(age, 18), max(id, 1), max(id) FROM users
-- inferred: tuple[int | None, int, int | None]
```

```sql
SELECT name, max(age), max(id) FROM users GROUP BY name
-- inferred: tuple[str, int | None, int]
```

A cast keeps the nullability of what it casts. A scalar subquery with no row is NULL.

```sql
SELECT (SELECT nick FROM users u WHERE u.id = users.id + 1), (SELECT count(*) FROM users) FROM users
-- inferred: tuple[str | None, int]
```

## MySQL WITH ROLLUP nulls the grouping columns

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE sales (id INT NOT NULL PRIMARY KEY, region VARCHAR(20) NOT NULL, amount INT NOT NULL);
```

```sql
SELECT CAST(region AS CHAR), count(*), sum(amount) FROM sales GROUP BY region WITH ROLLUP
-- inferred: tuple[str | None, int, int | None]
```

## Division and modulo by zero are NULL in MySQL and SQLite

PostgreSQL raises an error on a division by zero. MySQL and SQLite return NULL, so the
result is optional even for NOT NULL operands, unless the divisor is a non-zero literal.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE m (id INT NOT NULL PRIMARY KEY, x INT NOT NULL, y INT NOT NULL);
```

```sql
SELECT x / y, x % y, x / 2, x % 2, id + y FROM m
-- inferred: tuple[decimal.Decimal | None, int | None, decimal.Decimal, int, int]
```

## SQLite division and modulo by zero

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE m (id INTEGER NOT NULL PRIMARY KEY, x INTEGER NOT NULL, y INTEGER NOT NULL);
```

```sql
SELECT x / y, x % y, x / 2, x % 2 FROM m
-- inferred: tuple[int | None, int | None, int, int]
```
