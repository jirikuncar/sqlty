# UUID Type Inference

## UUID with other types

```sql
SELECT user_id::UUID, name::TEXT FROM users
-- inferred: tuple[uuid.UUID, str]
```
