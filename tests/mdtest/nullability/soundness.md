# Nullability: soundness, no false non-null (PostgreSQL)

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
    nums INTEGER[] NOT NULL,
    doc JSONB NOT NULL
);
CREATE TABLE u (id INTEGER PRIMARY KEY, a INTEGER);
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

## CASE: ELSE NULL stays NULL under a parameter in WHERE

A parameter in `WHERE` proves the column it is compared with, not the `NULL` of an
`ELSE` branch.

```sql
SELECT CASE WHEN a > 0 THEN 5::float / a ELSE NULL END AS ratio FROM t
-- inferred: tuple[float | None]
```

```sql
SELECT CASE WHEN a > 0 THEN 5::float / a ELSE NULL END AS ratio FROM t WHERE id = %(id)s
-- inferred: tuple[float | None]
```

```sql
SELECT CASE WHEN a > 0 THEN 5::float / a ELSE NULL END AS ratio FROM t WHERE id = %s
-- inferred: tuple[float | None]
```

```sql
SELECT CASE WHEN a > 0 THEN a END, a FROM t WHERE a = %(a)s
-- inferred: tuple[int | None, int]
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

## The pgvector distance operator is strict

`<=>` is null-safe equality only in MySQL. In PostgreSQL it is a strict operator, NULL for a
NULL operand.

```sql
SELECT CAST(a <=> g AS TEXT), CAST(g <=> g AS TEXT) FROM t
-- inferred: tuple[str | None, str]
```

## Array elements can be NULL

A NOT NULL array still holds NULL elements, so a comparison against them can be NULL.

```sql
SELECT g = ANY (nums), g = ALL (nums), unnest(nums) FROM t
-- inferred: tuple[bool | None, bool | None, int | None]
```

```sql
SELECT g IN (SELECT unnest(nums)) FROM t
-- inferred: tuple[bool | None]
```

An array spelled out lists its elements.

```sql
SELECT g = ANY (ARRAY[1, 2]), g = ANY (ARRAY[1, a]) FROM t
-- inferred: tuple[bool, bool | None]
```

## Functions that return NULL for a non-null input

`substring(s FROM 'pattern')` is NULL when nothing matches, and `current_setting(name, true)`
when the setting is not there. The integer forms stay non-null.

```sql
SELECT substring(txt FROM 'a+'), substring(txt FROM 2 FOR 3), substring(txt FROM 2), current_setting('x', true), current_setting('x') FROM t
-- inferred: tuple[str | None, str, str, str | None, str]
```

## Functions that look through NULLs do not narrow their arguments

`num_nulls`, `num_nonnulls` and the JSON builders accept NULL arguments, so a non-null result
says nothing about them.

```sql
SELECT a FROM t WHERE num_nonnulls(a, g) IS NOT NULL
-- inferred: tuple[int | None]
```

```sql
SELECT a FROM t WHERE jsonb_build_array(a) IS NOT NULL
-- inferred: tuple[int | None]
```

```sql
SELECT a FROM t WHERE jsonb_build_object('k', a) IS NOT NULL
-- inferred: tuple[int | None]
```

`array_agg` of a group with a NULL is still an array.

```sql
SELECT a FROM t GROUP BY a HAVING array_agg(a) IS NOT NULL
-- inferred: tuple[int | None]
```

A strict function still narrows.

```sql
SELECT a FROM t WHERE abs(a) IS NOT NULL
-- inferred: tuple[int]
```

## Recursive CTEs: non-null in the anchor and in every recursive step

The recursive term reads the rows found so far, so a column it fills from a nullable column
is nullable for the next step, too.

```sql
WITH RECURSIVE r(x, y) AS (
    SELECT 1, 1
    UNION ALL
    SELECT y, NULL FROM r WHERE x < 5
)
SELECT x, y FROM r
-- inferred: tuple[int | None, int | None]
```

A shift that spreads NULL over several rounds is followed to the end.

```sql
WITH RECURSIVE r(x, y, z) AS (
    SELECT 1, 1, 1
    UNION ALL
    SELECT y, z, NULL FROM r WHERE x < 5
)
SELECT x, y, z FROM r
-- inferred: tuple[int | None, int | None, int | None]
```

A column that stays non-null in both terms is non-null.

```sql
WITH RECURSIVE r(n, m) AS (
    SELECT 1, 1
    UNION ALL
    SELECT n + 1, m FROM r WHERE n < 5
)
SELECT n, m FROM r
-- inferred: tuple[int, int]
```

## A nested SELECT does not inherit conditions on expressions it rebinds

`t.a + 1 > 0` rules out NULL for the outer `t`, but the subquery's own `t` is another table.

```sql
SELECT t.id IN (SELECT t.a + 1 FROM u t) FROM t WHERE t.a + 1 > 0
-- inferred: tuple[bool | None]
```

```sql
SELECT t.id IN (SELECT t.a + 1 FROM u t) FROM t
-- inferred: tuple[bool | None]
```

## Narrowing follows the column's identity, not its spelling

A quoted mixed-case column is narrowed by a condition on it.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    "userId" INTEGER,
    "Email" TEXT
);
```

```sql
SELECT "userId", "Email" FROM accounts WHERE "userId" IS NOT NULL
-- inferred: tuple[int, str | None]
```

```sql
SELECT a."userId" FROM accounts AS a WHERE a."userId" = 1
-- inferred: tuple[int]
```

A left-joined relation with a quoted alias is back once a condition proves its row.

```sql
SELECT "A".id FROM accounts LEFT JOIN accounts AS "A" ON "A"."userId" = accounts.id WHERE "A"."userId" IS NOT NULL
-- inferred: tuple[int]
```
