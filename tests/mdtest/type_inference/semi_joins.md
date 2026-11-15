# Semi and anti joins

A semi join keeps the rows of one side that have a match on the other, an anti join
those that have none: `*` lists that side's columns only. ClickHouse lists both sides
(`semi_join_include_columns_from_both_sides` is on by default).

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "duckdb"
```

`schema.sql`:

```sql
CREATE TABLE a (id INT NOT NULL, x INT);
CREATE TABLE b (id INT NOT NULL, y INT);
```

## DuckDB

```sql
SELECT * FROM a SEMI JOIN b USING (id)
-- columns: id: int (INT)
-- columns: x: int | None (INT)
```

```sql
SELECT * FROM a ANTI JOIN b ON a.id = b.id
-- columns: id: int (INT)
-- columns: x: int | None (INT)
```

## Databricks: a right semi or anti join keeps the right side

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "databricks"
```

```sql
SELECT * FROM a LEFT SEMI JOIN b ON a.id = b.id
-- columns: id: int (INT)
-- columns: x: int | None (INT)
```

```sql
SELECT * FROM a RIGHT ANTI JOIN b USING (id)
-- columns: id: int (INT)
-- columns: y: int | None (INT)
```

## ClickHouse lists both sides

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "clickhouse"
```

```sql
SELECT * FROM a LEFT SEMI JOIN b USING (id)
-- columns: id: int (INT)
-- columns: x: int | None (INT)
-- columns: y: int | None (INT)
```

```sql
SELECT * FROM a LEFT ANTI JOIN b ON a.id = b.id
-- columns: id: int (INT)
-- columns: x: int | None (INT)
-- columns: id: int (INT)
-- columns: y: int | None (INT)
```

## An ASOF join pads the right side with nulls

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "snowflake"
```

```sql
SELECT * FROM a ASOF JOIN b MATCH_CONDITION (a.x >= b.y) USING (id)
-- columns: id: int (INT)
-- columns: x: int | None (INT)
-- columns: y: int | None (INT)
```
