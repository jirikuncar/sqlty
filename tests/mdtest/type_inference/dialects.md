# Dialect-specific Type Inference

## PostgreSQL integer cast

```toml
[tool.sqlty]
dialect = "postgres"
```

```sql
SELECT id::INTEGER FROM users
-- inferred: tuple[int]
```

## MySQL SIGNED cast

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CAST(id AS SIGNED) FROM users
-- inferred: tuple[int]
```

## SQLite integer

```toml
[tool.sqlty]
dialect = "sqlite"
```

```sql
SELECT CAST(id AS INTEGER) FROM users
-- inferred: tuple[int]
```
