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

## Manual SQL[CustomType] without --sql is not rewritten

User-defined `SQL[T]` wrappers without `--sql` prefix are left untouched:

```python
q = SQL[User]("SELECT id, name FROM users")
# rewritten: q = SQL[User]("SELECT id, name FROM users")
```

## Manual SQL[CustomType] with --sql is not double-wrapped

Already-wrapped `--sql` strings are detected by the existing `SQL[` wrapper
and skipped regardless of the type parameter:

```python
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten: q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Multiline SQL[CustomType] is not rewritten

```python
q = SQL[User]("""--sql
SELECT id::INTEGER, name::TEXT FROM users""")
# rewritten: q = SQL[User]("""--sql
# rewritten: SELECT id::INTEGER, name::TEXT FROM users""")
```

## Non-SQL code is unchanged

```python
x = 42
# rewritten: x = 42
```
