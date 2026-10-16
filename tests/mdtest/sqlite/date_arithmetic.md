# SQLite date arithmetic

SQLite stores dates as text or numbers and has no `INTERVAL`, so arithmetic on
a date column is left untyped; date functions such as `date()` do the work.

`schema.sql`:

```sql
CREATE TABLE events (d DATE NOT NULL, n INTEGER NOT NULL);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

```sql
SELECT d + n AS a, d - d AS b, n + n AS c FROM events
-- columns: a: None (None)
-- columns: b: None (None)
-- columns: c: int (INT)
```
