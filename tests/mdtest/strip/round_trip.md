# Strip: Round-Trip Invariants

## Strip removes SQL wrapper

```python
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# stripped: q = "--sql\nSELECT id::INTEGER FROM users"
```

## SQL[CustomType] without --sql survives both rewrite and strip

Custom `SQL[T]` wrappers without `--sql` prefix are never touched:

```python
q = SQL[User]("SELECT id, name FROM users")
# rewritten: q = SQL[User]("SELECT id, name FROM users")
# stripped: q = SQL[User]("SELECT id, name FROM users")
```

## Space form strip still works on manually wrapped strings

```python
q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
# stripped: q = "--sql SELECT id::INTEGER FROM users"
```

## Triple-quoted multiline round-trip: rewrite then strip

### Rewrite

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten: q = SQL[tuple[int]]("""--sql
# rewritten: SELECT id::INTEGER FROM users""")
```

### Strip

```python
q = SQL[tuple[int]]("""--sql
SELECT id::INTEGER FROM users""")
# stripped: q = """--sql
# stripped: SELECT id::INTEGER FROM users"""
```
