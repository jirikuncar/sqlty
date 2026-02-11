# Lint Rule Configuration

## Default config — star is warned

```sql
SELECT * FROM users
```

```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
```

## All rules disabled — no diagnostics

```toml
[sqlty]
[sqlty.rules]
select-star = "ignore"
select-star-no-schema = "ignore"
```

```sql
SELECT * FROM users
```

```python
# error:
```

## Star rule set to error

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[sqlty]
schema = "schema.sql"
[sqlty.rules]
select-star = "error"
```

```sql
SELECT * FROM users
```

```python
# error: [select-star] "SELECT * used — expand to explicit column list"
```
