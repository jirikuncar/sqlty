# Dialect-specific Type Inference

## PostgreSQL integer cast

```toml
[sqlty]
dialect = "postgres"
```

```sql
SELECT id::INTEGER FROM users
```

```python
# inferred: tuple[int]
```

## MySQL SIGNED cast

```toml
[sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED) FROM users
```

```python
# inferred: tuple[int]
```

## SQLite integer

```toml
[sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(id AS INTEGER) FROM users
```

```python
# inferred: tuple[int]
```
