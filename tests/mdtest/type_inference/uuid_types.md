# UUID Type Inference

## UUID type

```sql
SELECT user_id::UUID FROM users
```

```python
# inferred: tuple[uuid.UUID]
```

## UUID with other types

```sql
SELECT user_id::UUID, name::TEXT FROM users
```

```python
# inferred: tuple[uuid.UUID, str]
```
