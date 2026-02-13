# SELECT * Checks

## Star with no schema is an error

```sql
SELECT * FROM users
```

```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
```

## Star with schema produces warning

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
SELECT * FROM users
```

```python
# error: [select-star] "SELECT * used — expand to explicit column list"
```

## Star with schema and rule disabled produces no error

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
[sqlty.rules]
select-star = "ignore"
```

```sql
SELECT * FROM users
```

```python
# error:
```

## Explicit columns produce no error

```sql
SELECT id::INTEGER, name::TEXT FROM users
```

```python
# error:
```
