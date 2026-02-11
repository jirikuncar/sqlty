# Rewrite Edge Cases

## Query with datetime type adds SQL wrapper

```python
q = "--sql SELECT created_at::TIMESTAMP FROM users"
# rewritten: q = SQL[tuple[datetime.datetime]]("--sql SELECT created_at::TIMESTAMP FROM users")
```

## Query with UUID type

```python
q = "--sql SELECT user_id::UUID FROM users"
# rewritten: q = SQL[tuple[uuid.UUID]]("--sql SELECT user_id::UUID FROM users")
```

## Invalid SQL wraps as Any

```python
q = "--sql NOT VALID SQL"
# rewritten: q = SQL[Any]("--sql NOT VALID SQL")
```

## Empty string is not rewritten

```python
q = ""
# rewritten: q = ""
```

## Multiple queries on separate lines

```python
q1 = "--sql SELECT id::INTEGER FROM users"
# rewritten: q1 = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
```

## Single-quoted string

```python
q = '--sql SELECT id::INTEGER FROM users'
# rewritten: q = SQL[tuple[int]]('--sql SELECT id::INTEGER FROM users')
```
