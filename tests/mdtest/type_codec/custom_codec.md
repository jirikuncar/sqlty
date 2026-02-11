# Custom TypeCodec via Template

Custom codecs use a template string with `{types}` placeholder.

## list template

```toml
[sqlty]
[sqlty.codec]
template = "list[{types}]"
```

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# inferred: list[int, str]
```

## Row template

```toml
[sqlty]
[sqlty.codec]
template = "Row[{types}]"
```

```sql
SELECT id::INTEGER, name::TEXT, active::BOOLEAN FROM users
```

```python
# inferred: Row[int, str, bool]
```

## Single column with custom template

```toml
[sqlty]
[sqlty.codec]
template = "Scalar[{types}]"
```

```sql
SELECT COUNT(*) FROM users
```

```python
# inferred: Scalar[int]
```

## Complex types with custom template

```toml
[sqlty]
[sqlty.codec]
template = "Record[{types}]"
```

```sql
SELECT
    id::INTEGER,
    created_at::TIMESTAMP,
    user_id::UUID,
    data::JSON
FROM events
```

```python
# inferred: Record[int, datetime.datetime, uuid.UUID, dict]
```
