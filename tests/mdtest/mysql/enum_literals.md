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
