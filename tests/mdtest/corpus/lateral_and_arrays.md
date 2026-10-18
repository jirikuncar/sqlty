# Corpus: LATERAL and arrays

A LATERAL subquery can read earlier FROM items; `CROSS JOIN LATERAL` and `JOIN LATERAL`
keep its nullability and `LEFT JOIN LATERAL` null-extends it. Array elements can be NULL
whatever the array's own constraint.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## CROSS and LEFT JOIN LATERAL referencing an outer column

The cross-joined subquery keeps its NOT NULL column, while the left-joined one finds no row
for the largest id.

`schema.sql`:

```sql
CREATE TABLE items (id INTEGER PRIMARY KEY, val INTEGER NOT NULL);
```

```sql
SELECT t.id, sub.result, nxt.next_id
FROM items t
CROSS JOIN LATERAL (SELECT t.val AS result FROM items LIMIT 1) sub
LEFT JOIN LATERAL (
    SELECT i.id AS next_id FROM items i WHERE i.id > t.id ORDER BY i.id LIMIT 1
) nxt ON true
-- inferred: tuple[int, int, int | None]
```

## LEFT JOIN LATERAL with a JSON builder

The null-extended row has NULL in every column, including the built object.

`schema.sql`:

```sql
CREATE TABLE parent_table (id INTEGER PRIMARY KEY);
CREATE TABLE child_table (parent_id INTEGER REFERENCES parent_table(id), status BOOLEAN NOT NULL);
```

```sql
SELECT parent_table.id, latest_child.status AS latest_status, latest_child.snapshot::text AS snapshot
FROM parent_table
LEFT JOIN LATERAL (
    SELECT child_table.status, jsonb_build_object('status', child_table.status) AS snapshot
    FROM child_table
    WHERE child_table.parent_id = parent_table.id
) latest_child ON TRUE
-- inferred: tuple[int, bool | None, str | None]
```

## INNER JOIN LATERAL with a JSON builder

An inner lateral join adds no NULLs, so the object is non-null and `nickname` keeps its own
nullability.

`schema.sql`:

```sql
CREATE TABLE customer (id INTEGER PRIMARY KEY, name TEXT NOT NULL, nickname TEXT);
CREATE TABLE invoice (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL);
```

```sql
SELECT inv.id, cust_json.snapshot::text AS snapshot, cust_json.nickname
FROM invoice inv
JOIN LATERAL (
    SELECT jsonb_build_object('name', c.name) AS snapshot, c.nickname
    FROM customer c
    WHERE c.id = inv.customer_id
) cust_json ON TRUE
-- inferred: tuple[int, str, str | None]
```

## unnest of parameter arrays

An array can hold NULL elements, and a multi-argument `unnest` pads the shorter array with
NULLs.

`schema.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, admin BOOLEAN NOT NULL);
```

```sql
SELECT * FROM unnest($1::BIGINT[], $2::BOOLEAN[]) AS t(user_id, admin)
-- inferred: tuple[int | None, bool | None]
```
