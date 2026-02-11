# Basic Strip

## Simple strip

```python
q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
# stripped: q = "--sql SELECT 1::INTEGER"
```

## Strip with multiple columns

```python
q = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# stripped: q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
```

## Strip is idempotent

A plain --sql string without SQL[T] wrapper is unchanged:

```python
q = "--sql SELECT 1::INTEGER"
# stripped: q = "--sql SELECT 1::INTEGER"
```

## Non-SQL code is unchanged

```python
x = 42
# stripped: x = 42
```

## Manual SQL[T] wrapper is not stripped

A manually written `SQL[T]()` without `--sql` prefix is left untouched:

```python
q = SQL[int]("SELECT 1")
# stripped: q = SQL[int]("SELECT 1")
```

## Manual SQL[T] with custom query is not stripped

```python
q = SQL[tuple[int, str]]("SELECT id, name FROM users WHERE active")
# stripped: q = SQL[tuple[int, str]]("SELECT id, name FROM users WHERE active")
```
