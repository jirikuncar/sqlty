# Nullability: no false non-null (PostgreSQL)

Where NULL can occur, the column is optional. These are the cases where a looser
reading would call a nullable value non-null.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE t (
    id INTEGER PRIMARY KEY,
    a INTEGER,
    g INTEGER NOT NULL,
    tags TEXT[] NOT NULL,
    doc JSONB NOT NULL
);
```

## CASE: a WHEN that does not match also covers NULL

`a > 5` is NULL for a NULL `a`, so the `ELSE` branch can still see a NULL.

```sql
SELECT CASE WHEN a > 5 THEN 1 ELSE a END FROM t
-- inferred: tuple[int | None]
```

A condition that cannot be NULL does narrow the later branches.

```sql
SELECT CASE WHEN a IS NULL THEN 0 ELSE a END FROM t
-- inferred: tuple[int]
```

The right side of an `OR` is read where the left one is false or NULL.

```sql
SELECT a > 5 OR a = 1, (a > 5 OR a IS NOT NULL) FROM t
-- inferred: tuple[bool | None, bool | None]
```

## ROLLUP, CUBE and GROUPING SETS null the grouping columns

A subtotal row has NULL where its grouping column was rolled up, even for a NOT NULL column.

```sql
SELECT g, count(*) FROM t GROUP BY ROLLUP (g)
-- inferred: tuple[int | None, int]
```

```sql
SELECT g, a FROM t GROUP BY CUBE (g, a)
-- inferred: tuple[int | None, int | None]
```

```sql
SELECT g, id FROM t GROUP BY GROUPING SETS ((g), (id))
-- inferred: tuple[int | None, int | None]
```

A column grouped outside the `ROLLUP` is in every row, and an aggregate reads the rows of
its group.

```sql
SELECT g, id, max(id) FROM t GROUP BY id, ROLLUP (g)
-- inferred: tuple[int | None, int, int | None]
```

`WHERE` runs before the subtotals are made, so it cannot rule out their NULL.

```sql
SELECT g FROM t WHERE g > 1 GROUP BY ROLLUP (g)
-- inferred: tuple[int | None]
```
