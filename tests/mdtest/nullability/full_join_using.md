# The merged column of a full join

A row of `a FULL JOIN b USING (id)` has a row of `a`, of `b`, or of both, so its `id`,
`COALESCE(a.id, b.id)`, is not NULL when neither column is.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE a (id INT NOT NULL, x INT);
CREATE TABLE b (id INT NOT NULL, y INT);
CREATE TABLE c (id INT, z INT);
```

## Both columns NOT NULL

```sql
SELECT * FROM a FULL JOIN b USING (id)
-- columns: id: int (INT)
-- columns: x: int | None (INT)
-- columns: y: int | None (INT)
```

```sql
SELECT * FROM a NATURAL FULL JOIN b
-- columns: id: int (INT)
-- columns: x: int | None (INT)
-- columns: y: int | None (INT)
```

## A nullable column

```sql
SELECT * FROM a FULL JOIN c USING (id)
-- columns: id: int | None (INT)
-- columns: x: int | None (INT)
-- columns: z: int | None (INT)
```

## Another outer join can still fill it with NULLs

```sql
SELECT * FROM c LEFT JOIN (a FULL JOIN b USING (id)) ON true
-- columns: id: int | None (INT)
-- columns: z: int | None (INT)
-- columns: id: int | None (INT)
-- columns: x: int | None (INT)
-- columns: y: int | None (INT)
```
