# Where a finding is underlined

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT);
```

## A plain string

```python
# error[20:24]: [unknown-column] "column `nope` does not exist in `users`"
q = "--sql\nSELECT nope FROM users"
```

## A t-string

The fields of a t-string are not in the query, so a finding underlines the whole string.

```python
n = 1
# error[5:42]: [unknown-column] "column `nope` does not exist in `users`"
q = t"--sql\nSELECT nope, {n} FROM users"
```

## A string right after a keyword

```python
def f(x):
    if x:
        # error: [unknown-column] "column `nope` does not exist in `users`"
        return"--sql\nSELECT nope FROM users"
    # error: [unknown-column] "column `nope` does not exist in `users`"
    return x if x else"--sql\nSELECT nope FROM users"
```
