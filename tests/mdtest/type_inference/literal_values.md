# Literal value sets

A constant is a `typing.Literal` of its value, and `CASE`, `COALESCE`, `UNION` and
`VALUES` over constants unite the values of their branches. An expression that computes a
new value (arithmetic, a function) is the plain type again, so a set is always a superset
of what the expression can return. A set of more than 32 values widens to `str` or `int`.

## Constants

```sql
SELECT 'paid' AS status, 3 AS n, -1 AS offset
-- columns: status: typing.Literal['paid'] (None)
-- columns: n: typing.Literal[3] (None)
-- columns: offset: typing.Literal[-1] (None)
```

Other constants keep their types.

```sql
SELECT 1.5 AS ratio, TRUE AS flag, NULL AS nothing
-- columns: ratio: decimal.Decimal (None)
-- columns: flag: bool (None)
-- columns: nothing: None (None)
```

## Quotes and backslashes in a string

```sql
SELECT 'it''s' AS quoted, 'a\b' AS slash
-- columns: quoted: typing.Literal['it\'s'] (None)
-- columns: slash: typing.Literal['a\\b'] (None)
```

## CASE over constants

```sql
SELECT CASE WHEN n > 0 THEN 'positive' WHEN n < 0 THEN 'negative' ELSE 'zero' END AS sign
FROM (SELECT 1 AS n) t
-- columns: sign: typing.Literal['positive', 'negative', 'zero'] (None)
```

A `CASE` without `ELSE` can also be NULL.

```sql
SELECT CASE WHEN n > 0 THEN 1 END AS flag FROM (SELECT 1 AS n) t
-- columns: flag: typing.Literal[1] | None (None)
```

A branch that is not a constant makes the plain type.

```sql
SELECT CASE WHEN n > 0 THEN 'big' ELSE name::TEXT END AS size
FROM (SELECT 1 AS n, 'x' AS name) t
-- columns: size: str (TEXT)
```

## COALESCE with constants

```sql
SELECT COALESCE(NULL, 'none') AS label, COALESCE(x::TEXT, 'none') AS other
FROM (SELECT 'a' AS x) t
-- columns: label: typing.Literal['none'] (None)
-- columns: other: str (TEXT)
```

## UNION is the union of the values

```sql
SELECT 'a' AS kind UNION ALL SELECT 'b' UNION ALL SELECT 'a'
-- columns: kind: typing.Literal['a', 'b'] (None)
```

A branch with no known values widens the column to its type.

```sql
SELECT 'a' AS kind UNION ALL SELECT kind::TEXT FROM (SELECT 'x' AS kind) t
-- columns: kind: str (TEXT)
```

An integer constant still widens a `smallint`, and unknown types still yield.

```sql
SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT n::BIGINT FROM (SELECT 3 AS n) t
-- columns: n: int (BIGINT)
```

## VALUES

```sql
SELECT * FROM (VALUES (1, 'a'), (2, 'b')) AS t (id, tag)
-- columns: id: typing.Literal[1, 2] (None)
-- columns: tag: typing.Literal['a', 'b'] (None)
```

## Subqueries and CTEs keep the values

```sql
WITH kinds AS (SELECT 'a' AS kind UNION ALL SELECT 'b')
SELECT kind FROM kinds
-- columns: kind: typing.Literal['a', 'b'] (None)
```

```sql
SELECT (SELECT 'x') AS one, k FROM (SELECT 7 AS k) t
-- columns: one: typing.Literal['x'] (None)
-- columns: k: typing.Literal[7] (None)
```

## Computing a value gives the plain type

```sql
SELECT n + 1 AS next, -n AS neg, abs(n) AS size, upper(s) AS up, s || 'x' AS joined
FROM (SELECT 1 AS n, 'a' AS s) t
-- columns: next: int (INT)
-- columns: neg: int (None)
-- columns: size: int (None)
-- columns: up: str (TEXT)
-- columns: joined: str (TEXT)
```

`SUM` of constants is a number, not a value from the set.

```sql
SELECT SUM(n) AS total, MAX(n) AS biggest FROM (SELECT 1 AS n) t
-- columns: total: int | None (BIGINT)
-- columns: biggest: typing.Literal[1] | None (None)
```

## More than 32 values widen

```sql
SELECT 1 AS n UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6
UNION SELECT 7 UNION SELECT 8 UNION SELECT 9 UNION SELECT 10 UNION SELECT 11 UNION SELECT 12
UNION SELECT 13 UNION SELECT 14 UNION SELECT 15 UNION SELECT 16 UNION SELECT 17
UNION SELECT 18 UNION SELECT 19 UNION SELECT 20 UNION SELECT 21 UNION SELECT 22
UNION SELECT 23 UNION SELECT 24 UNION SELECT 25 UNION SELECT 26 UNION SELECT 27
UNION SELECT 28 UNION SELECT 29 UNION SELECT 30 UNION SELECT 31 UNION SELECT 32
UNION SELECT 33
-- columns: n: int (None)
```

```sql
SELECT 1 AS n UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6
UNION SELECT 7 UNION SELECT 8 UNION SELECT 9 UNION SELECT 10 UNION SELECT 11 UNION SELECT 12
UNION SELECT 13 UNION SELECT 14 UNION SELECT 15 UNION SELECT 16 UNION SELECT 17
UNION SELECT 18 UNION SELECT 19 UNION SELECT 20 UNION SELECT 21 UNION SELECT 22
UNION SELECT 23 UNION SELECT 24 UNION SELECT 25 UNION SELECT 26 UNION SELECT 27
UNION SELECT 28 UNION SELECT 29 UNION SELECT 30 UNION SELECT 31 UNION SELECT 32
-- columns: n: typing.Literal[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32] (None)
```

## Rewrite spells the values

```python
q = "--sql\nSELECT 'paid' AS status, 1 AS n"
# rewritten: q = SQL[tuple[typing.Literal['paid'], typing.Literal[1]]]("--sql\nSELECT 'paid' AS status, 1 AS n")
```

## Dialects

The values follow the dialect's rules for the type of a branch: PostgreSQL's `CASE`
unifies all its branches, other dialects take the first one's type, and the values still
cover every branch.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CASE WHEN n > 0 THEN 'a' ELSE 'b' END AS kind, COALESCE(NULL, 'c') AS other
FROM (SELECT 1 AS n) t
-- columns: kind: typing.Literal['a', 'b'] (None)
-- columns: other: typing.Literal['c'] (None)
```
