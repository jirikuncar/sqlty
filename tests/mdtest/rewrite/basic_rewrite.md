# Basic Rewrite

## Simple integer query

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: from sqlty import NoParams
# rewritten: q = SQL[tuple[int], NoParams]("--sql\nSELECT id::INTEGER FROM users")
```

## Multiple columns

```python
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# rewritten: from sqlty import NoParams
# rewritten: q = SQL[tuple[int, str], NoParams]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import NoParams
# rewritten-file: q = SQL[tuple[int, str], NoParams]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Non-SQL string is not rewritten

```python
q = "Hello, World!"
# rewritten: q = "Hello, World!"
```

## String without --sql prefix is not rewritten

```python
q = "SELECT 1::INTEGER"
# rewritten: q = "SELECT 1::INTEGER"
```

## Marked string joined with `+` is not rewritten

The query is the whole concatenation, so wrapping its first literal would be wrong.

```python
q = "--sql\nSELECT id::INTEGER FROM users" + " WHERE id = 1"
r = "SELECT id::INTEGER FROM users WHERE " + "--sql\nid = 1"
# rewritten: q = "--sql\nSELECT id::INTEGER FROM users" + " WHERE id = 1"
# rewritten: r = "SELECT id::INTEGER FROM users WHERE " + "--sql\nid = 1"
```

```python
# error:
q = "--sql\nSELECT id FROM users WHERE " + where
```

## Idempotent rewrite

Rewriting already-rewritten source produces no changes:

```python
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten: from sqlty import NoParams
# rewritten: q = SQL[tuple[int], NoParams]("--sql\nSELECT 1::INTEGER")
```
