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

## A CHECK that ties columns to a discriminator

A tagged union stored as a discriminator and per-variant columns: the `CHECK` lists, for
each value of `kind`, which columns are set. A `WHERE` or `CASE` branch that picks a value
of `kind` leaves only its arm, so the columns that arm requires are non-null.

`schema.sql`:

```sql
CREATE TYPE pay_kind AS ENUM ('card', 'iban', 'cash');
CREATE TABLE payments (
    id SERIAL PRIMARY KEY,
    kind pay_kind NOT NULL,
    card_last4 TEXT,
    iban TEXT,
    CHECK ((kind = 'card' AND card_last4 IS NOT NULL AND iban IS NULL)
        OR (kind IN ('iban', 'cash') AND iban IS NOT NULL AND card_last4 IS NULL))
);
CREATE TABLE refunds (id SERIAL PRIMARY KEY, payment_id INTEGER);
CREATE TABLE loose (id SERIAL PRIMARY KEY, kind TEXT, card_last4 TEXT, CHECK (kind = 'card' AND card_last4 IS NOT NULL OR kind = 'iban'));
CREATE TABLE other (
    id SERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    a TEXT,
    CHECK (kind = 'a' AND length(a) > 0 OR kind = 'b'),
    CHECK ((kind = 'a') = (a IS NOT NULL) OR kind > 'b')
);
```

### WHERE

```sql
SELECT card_last4, iban FROM payments WHERE kind = 'card'
-- inferred: tuple[str, str | None]
```

```sql
SELECT p.iban FROM payments p WHERE p.kind IN ('cash', 'iban') AND p.id > 1
-- inferred: tuple[str]
```

```sql
SELECT card_last4 FROM payments WHERE kind <> 'iban' AND kind <> 'cash'
-- inferred: tuple[str]
```

### CASE

```sql
SELECT CASE kind WHEN 'card' THEN card_last4 ELSE iban END AS account FROM payments
-- inferred: tuple[str]
```

```sql
SELECT CASE WHEN kind = 'card' THEN card_last4 ELSE 'none' END AS last4 FROM payments
-- inferred: tuple[str]
```

### What is not narrowed

Without a condition on `kind`, under `OR` that keeps both arms, on the side an outer join
null-extends (even where `WHERE` drops its NULL rows), or where the `CHECK` can be NULL
(`loose.kind` is nullable), the columns keep their declared nullability.

```sql
SELECT card_last4 FROM payments
-- inferred: tuple[str | None]
```

```sql
SELECT iban FROM payments WHERE kind = 'card' OR kind = 'iban'
-- inferred: tuple[str | None]
```

```sql
SELECT p.card_last4 FROM refunds r LEFT JOIN payments p ON p.id = r.payment_id WHERE p.kind = 'card'
-- inferred: tuple[str | None]
```

```sql
SELECT p.card_last4 FROM refunds r LEFT JOIN payments p ON p.id = r.payment_id AND p.kind = 'card'
-- inferred: tuple[str | None]
```

```sql
SELECT CASE WHEN kind = 'card' THEN iban ELSE '-' END AS iban FROM payments
-- inferred: tuple[str | None]
```

```sql
SELECT card_last4 FROM loose WHERE kind = 'card'
-- inferred: tuple[str | None]
```

A `CHECK` of another shape, a function or another operator, is not read.

```sql
SELECT a FROM other WHERE kind = 'a'
-- inferred: tuple[str | None]
```

### In Python

`sqlty rewrite` writes the narrowed type, and `sqlty check` reports a `SQL[tuple[...]]`
that still says `None` for a column the arm requires.

```python
cards = "--sql\nSELECT card_last4 FROM payments WHERE kind = 'card'"
# rewritten: cards = SQL[tuple[str]]("--sql\nSELECT card_last4 FROM payments WHERE kind = 'card'")
```

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str | None]] should be SQL[tuple[str]]"
cards = SQL[tuple[str | None]]("--sql\nSELECT card_last4 FROM payments WHERE kind = 'card'")
```
