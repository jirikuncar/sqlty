# Name resolution

## A user table named like a system catalog

A table the schema defines always resolves, whatever its name starts with.

### postgres

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INT, total INT);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int, int]
```

### mysql

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INT, total INT);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int, int]
```

### sqlite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE pg_orders (id INT, total INT);
```

```sql
SELECT id, total FROM pg_orders
-- inferred: tuple[int, int]
```
