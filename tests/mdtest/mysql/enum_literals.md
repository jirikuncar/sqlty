# MySQL ENUM and CHECK values

A column declared `ENUM('a', 'b')` is a `typing.Literal` of its labels, with the label
case kept. MySQL before 8.0.16 parses `CHECK` and does not enforce it, so a `CHECK` does
not limit the values of a column.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## ENUM columns

`schema.sql`:

```sql
CREATE TABLE orders (
    id INT PRIMARY KEY,
    status ENUM('Pending', 'paid', 'it''s') NOT NULL,
    kind ENUM('a', 'b'),
    plan VARCHAR(10) CHECK (plan IN ('free', 'pro')),
    level INT,
    CHECK (level IN (1, 2))
);
```

```sql
SELECT status, kind, plan, level FROM orders
-- columns: status: typing.Literal['Pending', 'paid', 'it\'s'] (ENUM('PENDING', 'PAID', 'IT''S'))
-- columns: kind: typing.Literal['a', 'b'] | None (ENUM('A', 'B'))
-- columns: plan: str | None (VARCHAR(10))
-- columns: level: int | None (INT)
```

Values carry through views, `CASE` and `UNION`.

```sql
SELECT kind FROM orders UNION SELECT 'c'
-- columns: kind: typing.Literal['a', 'b', 'c'] | None (ENUM('A', 'B'))
```

```sql
SELECT CASE WHEN id > 1 THEN 'x' ELSE 'y' END AS k, COALESCE(kind, 'z') AS z FROM orders
-- columns: k: typing.Literal['x', 'y'] (None)
-- columns: z: typing.Literal['a', 'b', 'z'] (ENUM('A', 'B'))
```

## Rewrite

```python
q = "-- sql\nSELECT status, kind FROM orders"
# rewritten: q = SQL[tuple[typing.Literal['Pending', 'paid', 'it\'s'], typing.Literal['a', 'b'] | None]]("-- sql\nSELECT status, kind FROM orders")
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INT PRIMARY KEY,
    status ENUM('Pending', 'paid', 'it''s') NOT NULL,
    kind ENUM('a', 'b')
);
```

## ALTER TABLE MODIFY and CHANGE

`MODIFY` and `CHANGE` replace a column's definition, its `ENUM` labels included.

`schema.sql`:

```sql
CREATE TABLE t (id INT PRIMARY KEY, a ENUM('x', 'y') NOT NULL);
CREATE TABLE u (id INT PRIMARY KEY, a ENUM('x', 'y') NOT NULL);
ALTER TABLE t MODIFY a ENUM('x', 'y', 'z') NOT NULL;
ALTER TABLE u ADD COLUMN b INT;
```

```sql
SELECT a FROM t
-- columns: a: typing.Literal['x', 'y', 'z'] (ENUM('X', 'Y', 'Z'))
```

```sql
SELECT a FROM u
-- columns: a: typing.Literal['x', 'y'] (ENUM('X', 'Y'))
```
