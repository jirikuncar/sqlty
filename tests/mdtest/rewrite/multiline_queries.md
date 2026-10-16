# Multiline Query Handling

## Single-line query with newline escape is rewritten

```python
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE active = true"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE active = true")
```

## Triple-quoted single-line string is rewritten

```python
q = """--sql\nSELECT id::INTEGER, name::TEXT FROM users"""
# rewritten: q = SQL[tuple[int, str]]("""--sql\nSELECT id::INTEGER, name::TEXT FROM users""")
```

## Triple-quoted multiline string is rewritten

```python
q = """--sql
SELECT id::INTEGER, name::TEXT FROM users"""
# rewritten: q = SQL[tuple[int, str]]("""--sql
# rewritten: SELECT id::INTEGER, name::TEXT FROM users""")
```

## Triple-quoted multiline with trailing newline is rewritten

```python
q = """--sql
SELECT id::INTEGER, name::TEXT
FROM users
WHERE active = true"""
# rewritten: q = SQL[tuple[int, str]]("""--sql
# rewritten: SELECT id::INTEGER, name::TEXT
# rewritten: FROM users
# rewritten: WHERE active = true""")
```

## Triple-quoted multiline whole-file includes imports

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("""--sql
# rewritten-file: SELECT id::INTEGER FROM users""")
```

## F-strings are not rewritten

F-strings cannot be statically analyzed, so they are left unchanged
and a warning is emitted. Set `fstring-skipped = "ignore"` in
`[tool.sqlty.rules]` to silence. For t-strings, use `tstring-skipped`.

```python
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = f"--sql\nSELECT {column}::INTEGER FROM users"
# rewritten: q = f"--sql\nSELECT {column}::INTEGER FROM users"
```

## T-strings (PEP 750)

A t-string whose fields are values is wrapped as `SQLTemplate[T]`: each `{field}`
is a parameter. A field with a format spec or a conversion, such as psycopg's
`{name:i}` identifier, changes the SQL itself, so that template is skipped:

```python
user_id = 1
column = "id"
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id}"
# error: [tstring-skipped] "t-string with --sql prefix skipped (a field with a format spec or conversion changes the SQL)"
r = t"--sql\nSELECT {column:i}::INTEGER FROM users"
# rewritten: user_id = 1
# rewritten: column = "id"
# rewritten: q = SQLTemplate[tuple[int, str]](t"--sql\nSELECT id::INTEGER, name::TEXT FROM users WHERE id = {user_id}")
# rewritten: r = t"--sql\nSELECT {column:i}::INTEGER FROM users"
```

## Silencing skipped f-strings

```toml
[tool.sqlty.rules]
fstring-skipped = "ignore"
```

```python
q = f"--sql\nSELECT {column}::INTEGER FROM users"
# error:
```

## Wrapper split across lines by a formatter is not wrapped again

```python
q = SQL[tuple[int]](
"--sql\nSELECT id::INTEGER FROM users"
)
# rewritten: q = SQL[tuple[int]](
# rewritten: "--sql\nSELECT id::INTEGER FROM users"
# rewritten: )
```
