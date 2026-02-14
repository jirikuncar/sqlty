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
