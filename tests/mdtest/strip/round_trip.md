# Strip: Round-Trip Invariants

## Rewrite produces SQL wrapper

```python
q = "--sql SELECT id::INTEGER FROM users"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Strip removes SQL wrapper

```python
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## Multi-column rewrite

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Multi-column strip

```python
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

## SQL[CustomType] survives rewrite round-trip

Rewrite does not modify an existing `SQL[User]` wrapper:

```python
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten: q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## SQL[CustomType] without --sql survives both rewrite and strip

Custom `SQL[T]` wrappers without `--sql` prefix are never touched:

```python
q = SQL[User]("SELECT id, name FROM users")
# rewritten: q = SQL[User]("SELECT id, name FROM users")
```

```python
q = SQL[User]("SELECT id, name FROM users")
# stripped: q = SQL[User]("SELECT id, name FROM users")
```

## Triple-quoted multiline round-trip: rewrite then strip

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten: q = SQL[tuple[int]]("""--sql
# rewritten: SELECT id::INTEGER FROM users""")
```

```python
q = SQL[tuple[int]]("""--sql
SELECT id::INTEGER FROM users""")
# stripped: q = """--sql
# stripped: SELECT id::INTEGER FROM users"""
```
