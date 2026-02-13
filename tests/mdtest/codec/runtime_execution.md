# Magic Codec Runtime Execution

## Integer query is transformed

```python
q = "--sql SELECT 1::INTEGER"
# decoded: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Multiple columns are transformed

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# decoded: q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

## Non-SQL strings are unchanged

```python
q = "hello world"
# decoded: q = "hello world"
```

## Invalid SQL is not transformed

```python
q = "--sql NOT VALID SQL"
# decoded: q = "--sql NOT VALID SQL"
```

## DML without RETURNING is not transformed

```python
q = "--sql UPDATE users SET name = 'x'"
# decoded: q = "--sql UPDATE users SET name = 'x'"
```
