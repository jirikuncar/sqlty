# PostgreSQL Aggregate Functions

```toml
[tool.sqlty]
dialect = "postgres"
```

## COUNT star returns int

```sql
SELECT COUNT(*) FROM users
-- inferred: tuple[int]
```

## COUNT DISTINCT returns int

```sql
SELECT COUNT(DISTINCT email)::INTEGER FROM users
-- inferred: tuple[int]
```

## SUM without cast or schema defaults to Decimal

```sql
SELECT SUM(amount) FROM orders
-- inferred: tuple[decimal.Decimal | None]
```

## AVG of an unknown column is Decimal

```sql
SELECT AVG(score) FROM tests
-- inferred: None
```

## SUM with explicit cast

```sql
SELECT SUM(quantity)::INTEGER FROM order_items
-- inferred: tuple[int | None]
```

## Mixed aggregates

```sql
SELECT
    COUNT(*)::INTEGER,
    SUM(total)::FLOAT,
    AVG(total)::FLOAT,
    MIN(created_at)::TIMESTAMP,
    MAX(created_at)::TIMESTAMP
FROM orders
-- inferred: tuple[int, float | None, float | None, datetime.datetime | None, datetime.datetime | None]
```

## GROUP BY with multiple aggregates

```sql
SELECT
    department::TEXT,
    COUNT(*)::INTEGER,
    AVG(salary)::FLOAT,
    MIN(hire_date)::DATE,
    MAX(salary)::FLOAT
FROM employees
GROUP BY department
HAVING COUNT(*) > 5
-- inferred: tuple[str, int, float, datetime.date, float]
```

## MIN and MAX of varchar are text

PostgreSQL has no `min` or `max` for `varchar`; it reads the column as `text`.
`char(n)` has its own.

`schema.sql`:

```sql
CREATE TABLE codes (v VARCHAR(20) NOT NULL, c CHAR(3) NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT max(v), min(v), max(c) FROM codes
-- columns: max: str | None (TEXT)
-- columns: min: str | None (TEXT)
-- columns: max: str | None (CHAR(3))
```
