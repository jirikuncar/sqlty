# Hover

`core.hover_source` says what the word at a cursor in a query string is, in Markdown. A
string gets hover exactly when `sqlty check` checks it. The `<CURSOR>` marks where the
cursor is; `# hover:` holds the contents, `None` for none.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total INTEGER);
CREATE VIEW user_names AS SELECT id, name FROM users;
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Column

A column shows its SQL type, whether it can be NULL, and the relation it comes from.

```python
q = "--sql\nSELECT name FROM users u WHERE u.em<CURSOR>ail IS NOT NULL"
# hover: Column `email` of `u`: `TEXT NULL`
```

## Table

A table or view lists its columns in the order the schema declares them.

```python
q = "--sql\nSELECT o.total FROM ord<CURSOR>ers o"
# hover: Table `public.orders`
# hover: - `id` `INT NOT NULL`
# hover: - `user_id` `INT NOT NULL`
# hover: - `total` `INT NULL`
```

```python
q = "--sql\nSELECT name FROM user_na<CURSOR>mes"
# hover: View `public.user_names`
# hover: - `id` `INT NOT NULL`
# hover: - `name` `TEXT NOT NULL`
```

## Alias

A word before `.` lists the columns of the relation it names.

```python
q = "--sql\nSELECT o.total FROM orders o WHERE <CURSOR>o.user_id = 1"
# hover: Relation `o`
# hover: - `id` `INT NOT NULL`
# hover: - `total` `INT NULL`
# hover: - `user_id` `INT NOT NULL`
```

## CTE

```python
q = "--sql\nWITH big AS (SELECT * FROM orders WHERE total > 100) SELECT id FROM b<CURSOR>ig"
# hover: CTE `big` of the query
```

## Result column

A word at the top level of a select item shows the result column: its Python type, its
SQL type and the inference rule behind it, as `sqlty check --explain` reads it.

```python
q = "--sql\nSELECT user_id, SUM(total) AS spe<CURSOR>nt FROM orders GROUP BY user_id"
# hover: Result column `spent`: `int | None` (`BIGINT`)
# hover:
# hover: Typed by `postgres.aggregate.sum-integers`: Summing small integers widens to bigint so the sum fits.
# hover:
# hover: PostgreSQL 17 documentation, 9.21, Table 9.60 General-Purpose Aggregate Functions (sum ( smallint ) → bigint, sum ( integer ) → bigint)
```

A name inside the expression is the column it names.

```python
q = "--sql\nSELECT user_id, SUM(tot<CURSOR>al) AS spent FROM orders GROUP BY user_id"
# hover: Column `total` of `orders`: `INT NULL`
```

### References of the dialect

A rule that holds for several databases cites each one's documentation. The hover keeps
the part of the configured dialect.

```python
q = "--sql\nSELECT total + 1 AS ne<CURSOR>xt FROM orders"
# hover: Result column `next`: `int | None` (`INT`)
# hover:
# hover: Typed by `common.arithmetic.numbers`: Arithmetic on two numbers gives the wider of their types.
# hover:
# hover: PostgreSQL 17 documentation, 9.3, Table 9.4 Mathematical Operators
```

#### MySQL

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "mysql"
```

```python
q = "--sql\nSELECT total + 1 AS ne<CURSOR>xt FROM orders"
# hover: Result column `next`: `int | None` (`INT`)
# hover:
# hover: Typed by `common.arithmetic.numbers`: Arithmetic on two numbers gives the wider of their types.
# hover:
# hover: MySQL 8.4 Reference Manual, 14.6.1 Arithmetic Operators
```

#### A dialect no reference cites

A dialect without documentation of its own, such as `generic`, gets every reference.

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "generic"
```

```python
q = "--sql\nSELECT total + 1 AS ne<CURSOR>xt FROM orders"
# hover: Result column `next`: `int | None` (`INT`)
# hover:
# hover: Typed by `common.arithmetic.numbers`: Arithmetic on two numbers gives the wider of their types.
# hover:
# hover: PostgreSQL 17 documentation, 9.3, Table 9.4 Mathematical Operators; MySQL 8.4 Reference Manual, 14.6.1 Arithmetic Operators; SQLite documentation, SQL Language Expressions, 2 Operators
```

## Annotated helper

A string passed to a parameter marked `Annotated[LiteralString, SQL]` is a query, with no
`--sql` marker.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T]) -> list[T]: ...

fetch_all("SELECT id, em<CURSOR>ail FROM users", tuple[int, str | None])
# hover: Result column `email`: `str | None` (`TEXT`)
```

## Nothing to show

A keyword, a name the schema does not define, and a string `sqlty check` does not read
show nothing.

```python
q = "--sql\nSEL<CURSOR>ECT name FROM users"
# hover: None
```

```python
q = "--sql\nSELECT name FROM nowh<CURSOR>ere"
# hover: None
```

```python
q = "SELECT name FROM us<CURSOR>ers"
# hover: None
```
