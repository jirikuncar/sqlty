# Extension types in other dialects

Types that PostgreSQL extensions define (`hstore`, ...) map to a Python type in PostgreSQL
only; elsewhere the name is an ordinary unknown type.

`schema.sql`:

```sql
CREATE TABLE items (id INTEGER PRIMARY KEY, tags HSTORE NOT NULL);
```

## SQLite

The name has SQLite's numeric affinity.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

```sql
SELECT tags FROM items
-- columns: tags: float (HSTORE)
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT tags FROM items
-- columns: tags: None (HSTORE)
```
