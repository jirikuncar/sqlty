# Rewrite Edge Cases

## Query with datetime type adds SQL wrapper

```python
q = "--sql\nSELECT created_at::TIMESTAMP FROM users"
# rewritten: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM users")
```

## Query with UUID type

```python
q = "--sql\nSELECT user_id::UUID FROM users"
# rewritten: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID FROM users")
```

## Invalid SQL is not wrapped

```python
q = "--sql\nNOT VALID SQL"
# rewritten: q = "--sql\nNOT VALID SQL"
```

## Empty string is not rewritten

```python
q = ""
# rewritten: q = ""
```

## Just --sql prefix with no query is not wrapped

```python
q = "--sql"
# rewritten: q = "--sql"
```

## UPDATE without RETURNING is not wrapped

```python
q = "--sql\nUPDATE users SET name = 'x'"
# rewritten: q = "--sql\nUPDATE users SET name = 'x'"
```

## INSERT without RETURNING is not wrapped

```python
q = "--sql\nINSERT INTO users (name) VALUES ('x')"
# rewritten: q = "--sql\nINSERT INTO users (name) VALUES ('x')"
```

## DELETE without RETURNING is not wrapped

```python
q = "--sql\nDELETE FROM users"
# rewritten: q = "--sql\nDELETE FROM users"
```

## DDL is not wrapped

```python
q = "--sql\nCREATE TABLE t (id INT)"
# rewritten: q = "--sql\nCREATE TABLE t (id INT)"
```

## DML with RETURNING is wrapped

```python
q = "--sql\nUPDATE users SET name = 'x' RETURNING id::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nUPDATE users SET name = 'x' RETURNING id::INTEGER")
```

## Multiple queries on separate lines

```python
q1 = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: q1 = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Single-quoted string

```python
q = '--sql\nSELECT id::INTEGER FROM users'
# rewritten: q = SQL[tuple[int]]('--sql\nSELECT id::INTEGER FROM users')
```

## Space after --sql makes entire line a SQL comment

`--sql SELECT ...` with a space is a SQL comment — the parser sees an
empty query, so the string is not wrapped:

```python
q = "--sql SELECT id::INTEGER FROM users"
# rewritten: q = "--sql SELECT id::INTEGER FROM users"
```

## Strings after an f-string or t-string are still wrapped

The scan moves past prefixed literals, so their closing quote is not paired
with the next string's opening quote.

```python
table = "users"
f = f"--sql\nSELECT id FROM {table}"
t = t'--sql\nSELECT id FROM {table}'
q = "--sql\nSELECT id::INTEGER FROM users"
r = '--sql\nSELECT name::TEXT FROM users'
# rewritten-file: from sqlty import SQL
# rewritten-file: table = "users"
# rewritten-file: f = f"--sql\nSELECT id FROM {table}"
# rewritten-file: t = t'--sql\nSELECT id FROM {table}'
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: r = SQL[tuple[str]]('--sql\nSELECT name::TEXT FROM users')
```

## Apostrophes in comments and escaped quotes hide no strings

A comment is skipped as a whole, and a backslash escapes the next character,
so neither opens a string that would swallow the next query.

```python
# it's a helper
a = 'it\'s'
q = "--sql\nSELECT id::INTEGER FROM users"
b = br'\'"'
r = '--sql\nSELECT name::TEXT FROM users'
# that's all
# rewritten-file: from sqlty import SQL
# rewritten-file: # it's a helper
# rewritten-file: a = 'it\'s'
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
# rewritten-file: b = br'\'"'
# rewritten-file: r = SQL[tuple[str]]('--sql\nSELECT name::TEXT FROM users')
# rewritten-file: # that's all
```

## A `u` string is wrapped like a plain one

```python
q = u"--sql\nSELECT id::INTEGER FROM users"
# rewritten: q = SQL[tuple[int]](u"--sql\nSELECT id::INTEGER FROM users")
```

```python
from sqlty import SQL

q = SQL[tuple[int]](u"--sql\nSELECT id::INTEGER FROM users")
# stripped: q = u"--sql\nSELECT id::INTEGER FROM users"
```
