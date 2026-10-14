# Literal value sets

A string constant column is a `typing.Literal` of its value (a bare integer such as
`SELECT 1` stays `int`), and `CASE`, `COALESCE`, `UNION` and
`VALUES` over constants unite the values of their branches. An expression that computes a
new value (arithmetic, a function) is the plain type again, so a set is always a superset
of what the expression can return. A set of more than 32 values widens to `str` or `int`.

## Constants

```sql
SELECT 'paid' AS status, 3 AS n, -1 AS offset
-- columns: status: typing.Literal['paid'] (None)
-- columns: n: int (None)
-- columns: offset: int (None)
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
-- columns: k: int (None)
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
-- columns: biggest: int | None (None)
```

## More than 32 values widen

```sql
SELECT 'v1' AS n UNION SELECT 'v2' UNION SELECT 'v3' UNION SELECT 'v4' UNION SELECT 'v5' UNION SELECT 'v6'
UNION SELECT 'v7' UNION SELECT 'v8' UNION SELECT 'v9' UNION SELECT 'v10' UNION SELECT 'v11' UNION SELECT 'v12'
UNION SELECT 'v13' UNION SELECT 'v14' UNION SELECT 'v15' UNION SELECT 'v16' UNION SELECT 'v17'
UNION SELECT 'v18' UNION SELECT 'v19' UNION SELECT 'v20' UNION SELECT 'v21' UNION SELECT 'v22'
UNION SELECT 'v23' UNION SELECT 'v24' UNION SELECT 'v25' UNION SELECT 'v26' UNION SELECT 'v27'
UNION SELECT 'v28' UNION SELECT 'v29' UNION SELECT 'v30' UNION SELECT 'v31' UNION SELECT 'v32'
UNION SELECT 'v33'
-- columns: n: str (None)
```

```sql
SELECT 'v1' AS n UNION SELECT 'v2' UNION SELECT 'v3' UNION SELECT 'v4' UNION SELECT 'v5' UNION SELECT 'v6'
UNION SELECT 'v7' UNION SELECT 'v8' UNION SELECT 'v9' UNION SELECT 'v10' UNION SELECT 'v11' UNION SELECT 'v12'
UNION SELECT 'v13' UNION SELECT 'v14' UNION SELECT 'v15' UNION SELECT 'v16' UNION SELECT 'v17'
UNION SELECT 'v18' UNION SELECT 'v19' UNION SELECT 'v20' UNION SELECT 'v21' UNION SELECT 'v22'
UNION SELECT 'v23' UNION SELECT 'v24' UNION SELECT 'v25' UNION SELECT 'v26' UNION SELECT 'v27'
UNION SELECT 'v28' UNION SELECT 'v29' UNION SELECT 'v30' UNION SELECT 'v31' UNION SELECT 'v32'
-- columns: n: typing.Literal['v1', 'v2', 'v3', 'v4', 'v5', 'v6', 'v7', 'v8', 'v9', 'v10', 'v11', 'v12', 'v13', 'v14', 'v15', 'v16', 'v17', 'v18', 'v19', 'v20', 'v21', 'v22', 'v23', 'v24', 'v25', 'v26', 'v27', 'v28', 'v29', 'v30', 'v31', 'v32'] (None)
```

## Rewrite spells the values

```python
q = "--sql\nSELECT 'paid' AS status, 1 AS n"
# rewritten: q = SQL[tuple[typing.Literal['paid'], int]]("--sql\nSELECT 'paid' AS status, 1 AS n")
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
