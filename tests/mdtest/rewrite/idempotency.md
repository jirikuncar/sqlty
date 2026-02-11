# Rewrite Idempotency

## Already rewritten source is unchanged

```python
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Already rewritten with multiple types

```python
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Non-SQL code is unchanged

```python
x = 42
# rewritten: x = 42
```
