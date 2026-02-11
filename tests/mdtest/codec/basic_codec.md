# Magic Codec

## Simple integer query

```python
q = "--sql SELECT 1::INTEGER"
# decoded: q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
```

## Multiple columns

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# decoded: q = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
```

## String without --sql prefix is unchanged

```python
q = "SELECT 1"
# decoded: q = "SELECT 1"
```

## Non-sql string is unchanged

```python
q = "just a normal string"
# decoded: q = "just a normal string"
```

## Datetime type triggers import

```python
q = "--sql SELECT created_at::TIMESTAMP"
# decoded: q = SQL[tuple[datetime.datetime]]("--sql SELECT created_at::TIMESTAMP")
```

## UUID type triggers import

```python
q = "--sql SELECT user_id::UUID"
# decoded: q = SQL[tuple[uuid.UUID]]("--sql SELECT user_id::UUID")
```
