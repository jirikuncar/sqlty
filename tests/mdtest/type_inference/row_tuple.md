# Row Tuples

Rows are typed as `tuple[T, ...]`, one element per column.

## Single column produces single-element tuple

```sql
SELECT id::INTEGER FROM users
-- inferred: tuple[int]
```

## Two columns

```sql
SELECT id::INTEGER, name::TEXT FROM users
-- inferred: tuple[int, str]
```

## Many columns

```sql
SELECT
    id::INTEGER,
    name::TEXT,
    price::FLOAT,
    active::BOOLEAN,
    created_at::TIMESTAMP
FROM products
-- inferred: tuple[int, str, float, bool, datetime.datetime]
```
