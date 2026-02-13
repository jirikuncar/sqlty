# Basic Rewrite

## Simple integer query

```python
q = "--sql SELECT id::INTEGER FROM users"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

## Multiple columns

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Whole file after rewrite:

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
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

## Idempotent rewrite

Rewriting already-rewritten source produces no changes:

```python
q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
# rewritten: q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
```
