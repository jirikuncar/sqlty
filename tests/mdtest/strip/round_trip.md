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
