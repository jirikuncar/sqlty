# CHECK constraints that rule out NULL

A `CHECK` can require some of a table's columns to be set. sqlty reads three
forms, alone or joined by `AND`:

- `num_nonnulls(a, b) = 1`, `>= 1` or `> 0`: at least one of the columns is
  set, so `COALESCE` over all of them is never NULL.
- `a IS NOT NULL OR b IS NOT NULL`: the same.
- `num_nulls(a, b) = 0` and `a IS NOT NULL`: each column is set, like
  `NOT NULL`.

`schema.sql`:

```sql
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    a_id INTEGER,
    b_id INTEGER,
    note TEXT,
    x INTEGER,
    y INTEGER,
    z INTEGER CHECK (z IS NOT NULL),
    CHECK (num_nonnulls(a_id, b_id) = 1),
    CHECK (x IS NOT NULL OR y IS NOT NULL)
);
CREATE TABLE owners (id INTEGER PRIMARY KEY, item_id INTEGER);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## COALESCE over a checked group

```sql
SELECT COALESCE(a_id, b_id) AS ref, COALESCE(b_id, a_id, 0) AS ref2, COALESCE(x, y) AS xy, z FROM items
-- inferred: tuple[int, int, int, int]
```

## The other column of a group where one is NULL

Where a condition proves the other columns of a group NULL, the one left is
set: in a `CASE` branch, or after `WHERE ... IS NULL`.

```sql
SELECT CASE WHEN a_id IS NOT NULL THEN a_id ELSE b_id END AS ref, CASE WHEN x IS NULL THEN y ELSE x END AS xy FROM items
-- inferred: tuple[int, int]
```

```sql
SELECT b_id FROM items WHERE a_id IS NULL
-- inferred: tuple[int]
```

```sql
SELECT CASE WHEN a_id IS NOT NULL THEN a_id ELSE x END AS r, b_id FROM items WHERE x IS NULL
-- inferred: tuple[int | None, int | None]
```

## Columns outside a group stay nullable

```sql
SELECT COALESCE(a_id, x) AS r, a_id, COALESCE(note, 'none') AS n FROM items
-- inferred: tuple[int | None, int | None, str]
```

## A null-extended row has every column NULL

```sql
SELECT COALESCE(i.a_id, i.b_id) AS ref FROM owners o LEFT JOIN items i ON i.id = o.item_id
-- inferred: tuple[int | None]
```

## Dropping a constraint forgets it

Constraints are not tracked by name, so dropping any of them drops them all.

`schema.sql`:

```sql
CREATE TABLE items (id INTEGER PRIMARY KEY, a_id INTEGER, b_id INTEGER, CHECK (num_nonnulls(a_id, b_id) >= 1));
ALTER TABLE items DROP CONSTRAINT items_check;
```

```sql
SELECT COALESCE(a_id, b_id) AS ref FROM items
-- inferred: tuple[int | None]
```
