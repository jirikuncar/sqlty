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
-- inferred: tuple[decimal.Decimal]
```

## AVG of an unknown column is Decimal

```sql
SELECT AVG(score) FROM tests
-- inferred: None
```

## SUM with explicit cast

```sql
SELECT SUM(quantity)::INTEGER FROM order_items
-- inferred: tuple[int]
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
-- inferred: tuple[int, float, float, datetime.datetime, datetime.datetime]
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
