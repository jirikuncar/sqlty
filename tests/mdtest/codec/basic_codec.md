# Magic Codec

## Simple integer query

```python
q = "--sql\nSELECT 1::INTEGER"
# decoded: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Multiple columns

```python
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
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
q = "--sql\nSELECT created_at::TIMESTAMP"
# decoded: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP")
```

## UUID type triggers import

```python
q = "--sql\nSELECT user_id::UUID"
# decoded: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID")
```
