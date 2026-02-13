# Multiline Query Handling

## Single-line query with newline escape is rewritten

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users WHERE active = true"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE active = true")
```

## F-strings are not rewritten

```python
q = f"--sql SELECT {column}::INTEGER FROM users"
# rewritten: q = fSQL[tuple[int]]("--sql\nSELECT {column}::INTEGER FROM users")
```
