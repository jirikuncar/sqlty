# Multiline Query Handling

## Single-line query with newline escape is rewritten

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users WHERE active = true"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE active = true")
```

## F-strings are not rewritten

F-strings cannot be statically analyzed, so they are left unchanged
and a warning is emitted. Set `fstring-skipped = "ignore"` in
`[tool.sqlty.rules]` to silence.

```python
q = f"--sql SELECT {column}::INTEGER FROM users"
# rewritten: q = f"--sql SELECT {column}::INTEGER FROM users"
# warning: line 1: f-string with --sql prefix skipped (f-strings cannot be statically rewritten) [fstring-skipped]
```
