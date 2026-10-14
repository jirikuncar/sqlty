# SQLite CHECK values

SQLite enforces `CHECK`, so a text or integer column limited by `CHECK (col IN (...))`
is a `typing.Literal` of the listed values.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## Column and table CHECK

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
    priority INTEGER CHECK (priority = 1 OR priority = 2),
    note TEXT,
    CHECK (note IN ('a', 'b') OR note IS NULL)
);
```

```sql
SELECT status, priority, note FROM orders
-- columns: status: typing.Literal['pending', 'paid'] (TEXT)
-- columns: priority: typing.Literal[1, 2] | None (INT)
-- columns: note: typing.Literal['a', 'b'] | None (TEXT)
```

## Views, CASE and UNION

Values carry through `CASE`, `UNION` and views.

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
CREATE VIEW paid AS SELECT id, status FROM orders;
```

```sql
SELECT status FROM paid UNION ALL SELECT CASE WHEN 1 THEN 'x' ELSE 'y' END
-- columns: status: typing.Literal['pending', 'paid', 'x', 'y'] (TEXT)
```

## Rewrite

```python
q = "--sql\nSELECT status FROM orders"
# rewritten: q = SQL[tuple[typing.Literal['pending', 'paid']]]("--sql\nSELECT status FROM orders")
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
```
