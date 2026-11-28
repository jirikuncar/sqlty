# Extension types in other dialects

Types that PostgreSQL extensions define (`hstore`, `ltree`, `vector`, ...) map to a Python type in PostgreSQL
only; elsewhere the name is an ordinary unknown type.

`schema.sql`:

```sql
CREATE TABLE items (id INTEGER PRIMARY KEY, tags HSTORE NOT NULL, path LTREE NOT NULL, embedding VECTOR(3) NOT NULL);
```

## SQLite

The name has SQLite's numeric affinity.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

```sql
SELECT tags, path, embedding FROM items
-- columns: tags: float (HSTORE)
-- columns: path: float (LTREE)
-- columns: embedding: float (VECTOR(3))
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

```sql
SELECT tags, path, embedding FROM items
-- columns: tags: None (HSTORE)
-- columns: path: None (LTREE)
-- columns: embedding: None (VECTOR(3))
```
