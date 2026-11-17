# Magic Codec Runtime Execution

## Non-SQL strings are unchanged

```python
q = "hello world"
# decoded: q = "hello world"
```

## DML without RETURNING is not transformed

```python
q = "--sql\nUPDATE users SET name = 'x'"
# decoded: q = "--sql\nUPDATE users SET name = 'x'"
```
