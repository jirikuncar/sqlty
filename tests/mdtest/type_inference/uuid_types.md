# UUID Type Inference

## UUID type

```sql
SELECT user_id::UUID FROM users
-- inferred: tuple[uuid.UUID]
```

## UUID with other types

```sql
SELECT user_id::UUID, name::TEXT FROM users
-- inferred: tuple[uuid.UUID, str]
```
