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

## A negated IN or ANY does not narrow

Over an empty subquery or array, `NOT IN` is true and `= ANY` is false for a NULL `a`.

```sql
SELECT a FROM t WHERE a NOT IN (SELECT g FROM t WHERE g > 100)
-- inferred: tuple[int | None]
```

```sql
SELECT a FROM t WHERE NOT (a = ANY (ARRAY[1, 2]))
-- inferred: tuple[int | None]
```

```sql
SELECT a FROM t WHERE NOT (a IN (SELECT g FROM t))
-- inferred: tuple[int | None]
```

The positive forms still narrow, and so does a list.

```sql
SELECT a, id FROM t WHERE a IN (SELECT g FROM t)
-- inferred: tuple[int, int]
```

```sql
SELECT a FROM t WHERE a = ANY (ARRAY[1, 2])
-- inferred: tuple[int]
```

```sql
SELECT a FROM t WHERE a NOT IN (1, 2)
-- inferred: tuple[int]
```

## Aggregates and window functions that can be NULL

A sample statistic of one row is NULL, however many rows the group has. Percentiles,
`corr`, `covar_*` and `regr_*` are optional for the same reason.

```sql
SELECT g, stddev(g), var_samp(g) FROM t GROUP BY g
-- inferred: tuple[int, decimal.Decimal | None, decimal.Decimal | None]
```

`first_value` and `last_value` are NULL when the frame is empty.

```sql
SELECT
    first_value(id) OVER (ORDER BY id),
    last_value(id) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING),
    first_value(id) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND 1 PRECEDING),
    last_value(id) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 2 FOLLOWING),
    first_value(a) OVER (ORDER BY id)
FROM t
-- inferred: tuple[int, int, int | None, int | None, int | None]
```

A frame named elsewhere is not read, so it may be empty.

```sql
SELECT last_value(id) OVER w FROM t WINDOW w AS (ORDER BY id)
-- inferred: tuple[int | None]
```

`lag` and `lead` are NULL past the edge unless they have a non-null default.

```sql
SELECT lag(id) OVER (ORDER BY id), lag(id, 1, 0) OVER (ORDER BY id), lead(id, 1, NULL) OVER (ORDER BY id), lead(a, 1, 0) OVER (ORDER BY id) FROM t
-- inferred: tuple[int | None, int, int | None, int | None]
```

## Division by zero raises, so it is not NULL

```sql
SELECT id / g, id % g FROM t
-- inferred: tuple[int, int]
```
