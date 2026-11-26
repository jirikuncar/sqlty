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

### Where WHERE makes the row present

A `WHERE` that a null-extended row fails, such as `p.kind = 'card'`, keeps only the rows
the join matched, so the `CHECK` holds for them. So does one that proves a nullable
discriminator set.

```sql
SELECT p.card_last4 FROM refunds r LEFT JOIN payments p ON p.id = r.payment_id WHERE p.kind = 'card'
-- inferred: tuple[str]
```

```sql
SELECT card_last4 FROM loose WHERE kind = 'card'
-- inferred: tuple[str]
```

### What is not narrowed

Without a condition on `kind`, under `OR` that keeps both arms, on the side an outer join
null-extends, or where the `CHECK` can be NULL (`loose.kind` is nullable, and `OR` keeps
its NULL rows), the columns keep their declared nullability.

```sql
SELECT card_last4 FROM payments
-- inferred: tuple[str | None]
```

```sql
SELECT iban FROM payments WHERE kind = 'card' OR kind = 'iban'
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
SELECT card_last4 FROM loose WHERE kind = 'card' OR id > 1
-- inferred: tuple[str | None]
```

```sql
SELECT p.card_last4 FROM refunds r LEFT JOIN payments p ON p.id = r.payment_id WHERE p.kind = 'card' OR r.id > 1
-- inferred: tuple[str | None]
```

A `CHECK` of another shape, a function or another operator, is not read.

```sql
SELECT a FROM other WHERE kind = 'a'
-- inferred: tuple[str | None]
```

### CASE and num_nonnulls forms

The same union can be written as a `CASE` on the discriminator, whose arms are the
conditions each value requires, or with `num_nonnulls` and `num_nulls` in each arm. A row
that matches no `WHEN` passes the `CHECK` when there is no `ELSE` (the `CASE` is NULL), or
takes the `ELSE`.

`forms.sql`:

```sql
CREATE TYPE pay_kind AS ENUM ('card', 'iban', 'cash');
CREATE TABLE simple (
    id SERIAL PRIMARY KEY,
    kind pay_kind NOT NULL,
    card_last4 TEXT,
    iban TEXT,
    CHECK (CASE kind WHEN 'card' THEN card_last4 IS NOT NULL AND iban IS NULL
                     WHEN 'iban' THEN iban IS NOT NULL AND card_last4 IS NULL
                     ELSE num_nulls(card_last4, iban) = 2 END)
);
CREATE TABLE searched (
    id SERIAL PRIMARY KEY,
    kind pay_kind NOT NULL,
    card_last4 TEXT,
    iban TEXT,
    CHECK (CASE WHEN kind = 'card' THEN card_last4 IS NOT NULL
                WHEN kind IN ('iban') THEN iban IS NOT NULL END)
);
CREATE TABLE counted (
    id SERIAL PRIMARY KEY,
    kind pay_kind NOT NULL,
    card_last4 TEXT,
    iban TEXT,
    CHECK ((kind = 'cash' AND num_nonnulls(card_last4, iban) = 0)
        OR (kind IN ('card', 'iban') AND num_nonnulls(card_last4, iban) = 1))
);
CREATE TABLE mixed (
    id SERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    a TEXT,
    b TEXT,
    CHECK (CASE WHEN kind = 'a' THEN a IS NOT NULL WHEN b = 'x' THEN b IS NOT NULL END),
    c TEXT,
    d TEXT,
    CHECK (CASE kind WHEN 'b' THEN length(b) > 0 END),
    CHECK (CASE lower(kind) WHEN 'c' THEN c IS NOT NULL END),
    CHECK (kind = 'd' AND num_nonnulls(a, b, c, d, id, kind) = 6 OR kind = 'e')
);
CREATE TABLE odd (
    id SERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    a TEXT,
    b TEXT,
    c TEXT,
    d TEXT,
    e TEXT,
    CHECK (CASE WHEN kind > 'a' THEN a IS NOT NULL END),
    CHECK (kind = 'a' AND num_nonnulls(b, 1) = 1 OR kind = 'b'),
    CHECK (kind = 'a' AND CURRENT_DATE = 1 AND c IS NOT NULL OR kind = 'b'),
    CHECK (kind = 'a' AND length(d) = 1 AND d IS NOT NULL OR kind = 'b'),
    CHECK (CASE kind
                  WHEN '0' THEN e IS NOT NULL
                  WHEN '1' THEN e IS NOT NULL
                  WHEN '2' THEN e IS NOT NULL
                  WHEN '3' THEN e IS NOT NULL
                  WHEN '4' THEN e IS NOT NULL
                  WHEN '5' THEN e IS NOT NULL
                  WHEN '6' THEN e IS NOT NULL
                  WHEN '7' THEN e IS NOT NULL
                  WHEN '8' THEN e IS NOT NULL
                  WHEN '9' THEN e IS NOT NULL
                  WHEN '10' THEN e IS NOT NULL
                  WHEN '11' THEN e IS NOT NULL
                  WHEN '12' THEN e IS NOT NULL
                  WHEN '13' THEN e IS NOT NULL
                  WHEN '14' THEN e IS NOT NULL
                  WHEN '15' THEN e IS NOT NULL
                  WHEN '16' THEN e IS NOT NULL
                  WHEN '17' THEN e IS NOT NULL
                  WHEN '18' THEN e IS NOT NULL
                  WHEN '19' THEN e IS NOT NULL
                  WHEN '20' THEN e IS NOT NULL
                  WHEN '21' THEN e IS NOT NULL
                  WHEN '22' THEN e IS NOT NULL
                  WHEN '23' THEN e IS NOT NULL
                  WHEN '24' THEN e IS NOT NULL
                  WHEN '25' THEN e IS NOT NULL
                  WHEN '26' THEN e IS NOT NULL
                  WHEN '27' THEN e IS NOT NULL
                  WHEN '28' THEN e IS NOT NULL
                  WHEN '29' THEN e IS NOT NULL
                  WHEN '30' THEN e IS NOT NULL
                  WHEN '31' THEN e IS NOT NULL
                  WHEN '32' THEN e IS NOT NULL
           END)
);
CREATE TABLE tagged (
    id SERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    a TEXT,
    CHECK (CASE kind WHEN 'a' THEN TRUE ELSE a IS NOT NULL END)
);
CREATE TABLE repeated (
    id SERIAL PRIMARY KEY,
    kind pay_kind NOT NULL,
    x TEXT,
    y TEXT,
    CHECK (CASE kind WHEN 'card' THEN x IS NOT NULL WHEN 'card' THEN y IS NOT NULL END)
);
```

```toml
[tool.sqlty]
schema = "forms.sql"
```

```sql
SELECT card_last4, iban FROM simple WHERE kind = 'card'
-- inferred: tuple[str, str | None]
```

```sql
SELECT CASE kind WHEN 'iban' THEN iban ELSE 'none' END AS iban FROM simple
-- inferred: tuple[str]
```

```sql
SELECT card_last4, iban FROM searched WHERE kind = 'card'
-- inferred: tuple[str, str | None]
```

```sql
SELECT iban FROM searched WHERE kind = 'iban'
-- inferred: tuple[str]
```

No `WHEN` matches `'cash'`, so the `CHECK` says nothing about it.

```sql
SELECT card_last4, iban FROM searched WHERE kind = 'cash'
-- inferred: tuple[str | None, str | None]
```

`num_nonnulls(card_last4, iban) = 1` holds when one of them is set: with the other NULL,
it is the one.

```sql
SELECT iban FROM counted WHERE kind = 'iban' AND card_last4 IS NULL
-- inferred: tuple[str]
```

```sql
SELECT iban FROM counted WHERE kind = 'iban'
-- inferred: tuple[str | None]
```

A `CASE` whose `WHEN`s test different columns, an expression or another operator, or
list a value twice, or has more than 32 `WHEN`s is not read. Neither is an arm of another
shape, another function, or a count over more than five columns or over something else.

```sql
SELECT a FROM mixed WHERE kind = 'a'
-- inferred: tuple[str | None]
```

```sql
SELECT b, c, d FROM mixed WHERE kind = 'b' OR kind = 'c' OR kind = 'd'
-- inferred: tuple[str | None, str | None, str | None]
```

```sql
SELECT c FROM mixed WHERE kind = 'c'
-- inferred: tuple[str | None]
```

```sql
SELECT d FROM mixed WHERE kind = 'd'
-- inferred: tuple[str | None]
```

```sql
SELECT a, b, c, d FROM odd WHERE kind = 'a'
-- inferred: tuple[str | None, str | None, str | None, str | None]
```

```sql
SELECT e FROM odd WHERE kind = '1'
-- inferred: tuple[str | None]
```

A `THEN TRUE` arm requires nothing, and the `ELSE` covers the other values.

```sql
SELECT a FROM tagged WHERE kind = 'b'
-- inferred: tuple[str]
```

```sql
SELECT x FROM repeated WHERE kind = 'card'
-- inferred: tuple[str | None]
```

### A CASE over every value of a nullable column

A row whose `mood` is NULL matches no `WHEN`: the `CASE` is NULL for it.

`people.sql`:

```sql
CREATE TYPE mood AS ENUM ('happy', 'sad');
CREATE TABLE people (id SERIAL PRIMARY KEY, mood mood);
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    status INTEGER NOT NULL,
    done_at TEXT,
    CHECK ((status = 0 AND done_at IS NULL) OR (status = 1 AND done_at IS NOT NULL))
);
```

```toml
[tool.sqlty]
schema = "people.sql"
```

```sql
SELECT CASE mood WHEN 'happy' THEN 1 WHEN 'sad' THEN 2 END AS score FROM people
-- inferred: tuple[typing.Literal[1, 2] | None]
```

A constant of another kind than the column (`'0'` for an integer) is compared as the
column's, so it keeps the other arm.

```sql
SELECT done_at FROM jobs WHERE status IN (1, '0')
-- inferred: tuple[str | None]
```

### A CHECK dropped with its column

PostgreSQL drops a `CHECK` with a column it reads, so a column added again, or renamed in
its place, is not narrowed by it.

`altered.sql`:

```sql
CREATE TYPE pay_kind AS ENUM ('card', 'iban');
CREATE TABLE payments (
    id SERIAL PRIMARY KEY,
    kind pay_kind NOT NULL,
    card_last4 TEXT,
    iban TEXT,
    CHECK ((kind = 'card' AND card_last4 IS NOT NULL) OR (kind = 'iban' AND iban IS NOT NULL))
);
ALTER TABLE payments DROP COLUMN card_last4;
ALTER TABLE payments ADD COLUMN card_last4 TEXT;
CREATE TABLE added (id SERIAL PRIMARY KEY, kind TEXT NOT NULL, a TEXT);
ALTER TABLE added ADD CONSTRAINT tied CHECK ((kind = 'a' AND a IS NOT NULL) OR kind = 'b');
```

```toml
[tool.sqlty]
schema = "altered.sql"
```

```sql
SELECT card_last4, iban FROM payments WHERE kind = 'iban'
-- inferred: tuple[str | None, str | None]
```

A `CHECK` added by `ALTER TABLE` narrows as one in `CREATE TABLE` does.

```sql
SELECT a FROM added WHERE kind = 'a'
-- inferred: tuple[str]
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
