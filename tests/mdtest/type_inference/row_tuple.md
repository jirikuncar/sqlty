# Row Tuples

Rows are typed as `tuple[T, ...]`, one element per column.

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
