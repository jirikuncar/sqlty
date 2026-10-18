# Corpus: common table expressions

A CTE's columns have the types and nullability of its query. A recursive CTE's column is
nullable when the anchor or any recursive step can produce NULL there.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Qualified CTE column compared with a parameter

The CTE name qualifies its own columns, and a parameter compared with one takes its type.

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY);
```

```sql
WITH user_ids AS (SELECT id FROM users)
SELECT * FROM user_ids WHERE user_ids.id = $1
-- inferred: tuple[int]
-- params: 1: int (SERIAL)
```

## Recursive CTE with a LATERAL step and TABLE

<!-- mdtest:xfail the TABLE name statement is not analyzed, the row type is None -->

Both terms produce only NOT NULL columns, `CROSS JOIN LATERAL` never null-extends, and
`TABLE cte` is `SELECT * FROM cte`.

`schema.sql`:

```sql
CREATE TABLE tickers (
    symbol TEXT NOT NULL,
    ts BIGINT NOT NULL,
    price NUMERIC NOT NULL,
    PRIMARY KEY (symbol, ts)
);
```

```sql
WITH RECURSIVE cte AS (
    (SELECT * FROM tickers ORDER BY symbol, ts DESC LIMIT 1)
    UNION ALL
    SELECT l.*
    FROM cte c
    CROSS JOIN LATERAL (
        SELECT * FROM tickers t
        WHERE t.symbol > c.symbol
        ORDER BY t.symbol, t.ts DESC
        LIMIT 1
    ) l
)
TABLE cte ORDER BY symbol
-- inferred: tuple[str, int, decimal.Decimal]
```

## Recursive walk over a primary key

The anchor and the recursive term both select the primary key, so the column is never NULL,
with `UNION ALL` or `UNION` and through `SELECT *`.

`schema.sql`:

```sql
CREATE TABLE clients (id TEXT PRIMARY KEY, name TEXT NOT NULL, parent_id TEXT REFERENCES clients(id));
```

```sql
WITH RECURSIVE client_tree AS (
    SELECT id FROM clients WHERE id = $1
    UNION ALL
    SELECT c.id FROM clients c JOIN client_tree ct ON c.parent_id = ct.id
)
SELECT id FROM client_tree
-- inferred: tuple[str]
```

```sql
WITH RECURSIVE ancestors AS (
    SELECT id FROM clients WHERE id = $1
    UNION
    SELECT child.id FROM clients child JOIN ancestors a ON a.id = child.parent_id
)
SELECT * FROM ancestors
-- inferred: tuple[str]
```

## Grouped array_agg in a CTE on the optional side

Each group's array is non-null inside the CTE, but the LEFT JOIN null-extends it.

`schema.sql`:

```sql
CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');
CREATE TABLE b (a_id INTEGER NOT NULL, b_id INTEGER NOT NULL, PRIMARY KEY (a_id, b_id));
```

```sql
WITH subquery AS (
    SELECT a_id, array_agg(b_id) AS list FROM b GROUP BY a_id
)
SELECT subquery.list
FROM a
LEFT JOIN subquery ON subquery.a_id = a.id
-- inferred: tuple[list[int] | None]
```
