# Schema Inference

Point sqlty at your `CREATE TABLE` statements and drop the `::TYPE` casts.
Runnable project: [`examples/02_schema_inference/`](../../examples/02_schema_inference/).

## With a schema

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total NUMERIC(10, 2) NOT NULL
);
```

`sqlty rewrite --schema other.sql .` overrides the configured file.

### Columns

```python
users = "--sql\nSELECT id, name, email FROM users"
# rewritten: users = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

### Joins and aliases

```python
orders = """--sql
SELECT o.id, u.name, o.total
FROM orders o JOIN users u ON u.id = o.user_id"""
# rewritten: orders = SQL[tuple[int, str, float]]("""--sql
# rewritten: SELECT o.id, u.name, o.total
# rewritten: FROM orders o JOIN users u ON u.id = o.user_id""")
```

### Casts win over the schema

```python
mixed = "--sql\nSELECT id::TEXT, name FROM users"
# rewritten: mixed = SQL[tuple[str, str]]("--sql\nSELECT id::TEXT, name FROM users")
```

### `SELECT *`

The row type covers every column of the table. `sqlty check` still flags the
star, since a schema change would silently change the row;
`sqlty.core.expand_star()` returns the explicit column list.

```sql
SELECT * FROM users
-- inferred: tuple[int, str, str, datetime.datetime]
-- expanded: SELECT id::INT, name::TEXT, email::VARCHAR(255), created_at::TIMESTAMP FROM users
```

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list"
q = SQL[tuple[int, str, str, datetime.datetime]]("--sql\nSELECT * FROM users")
```

## Without a schema

Uncast columns of unknown tables are typed `str`:

```sql
SELECT id, name FROM users
-- inferred: tuple[str, str]
```
