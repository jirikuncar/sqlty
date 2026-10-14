# Literal values from CHECK constraints and domains

A column limited by `CHECK (col IN (...))`, `CHECK (col = 'a' OR col = 'b')` or
`CHECK (col = ANY (ARRAY[...]))` is a `typing.Literal` of the listed values when it is a
text or integer column. So is a column of a domain with such a `CHECK (VALUE IN (...))`.
The SQL type stays the column's own. A `CHECK` that is not a plain list of constants
gives no values, and neither does a list of more than 32.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Column CHECK

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded')),
    priority INTEGER CHECK (priority IN (1, 2, 3)),
    channel VARCHAR(10) CHECK (channel = 'web' OR channel = 'app' OR channel IS NULL),
    note TEXT
);
```

```sql
SELECT status, priority, channel, note FROM orders
-- columns: status: typing.Literal['pending', 'paid', 'refunded'] (TEXT)
-- columns: priority: typing.Literal[1, 2, 3] | None (INT)
-- columns: channel: typing.Literal['web', 'app'] | None (VARCHAR(10))
-- columns: note: str | None (TEXT)
```

## ANY (ARRAY[...]), with the casts pg_dump writes

`schema.sql`:

```sql
CREATE TABLE tickets (
    id INTEGER PRIMARY KEY,
    state TEXT CHECK (state = ANY (ARRAY['open', 'closed'])),
    kind VARCHAR(8) CHECK (((kind)::text = ANY ((ARRAY['bug'::character varying, 'task'::character varying])::text[])))
);
```

```sql
SELECT state, kind FROM tickets
-- columns: state: typing.Literal['open', 'closed'] | None (TEXT)
-- columns: kind: typing.Literal['bug', 'task'] | None (VARCHAR(8))
```

## Table CHECK, and a CHECK of several conditions

`schema.sql`:

```sql
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    state TEXT,
    attempts INTEGER,
    CONSTRAINT jobs_state CHECK (state IN ('queued', 'running', 'done')),
    CHECK (attempts >= 0 AND attempts IN (0, 1, 2))
);
```

```sql
SELECT state, attempts FROM jobs
-- columns: state: typing.Literal['queued', 'running', 'done'] | None (TEXT)
-- columns: attempts: typing.Literal[0, 1, 2] | None (INT)
```

## Two CHECKs on a column

Two `CHECK`s on a column leave the values both allow.

`schema.sql`:

```sql
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    state TEXT CHECK (state IN ('queued', 'running', 'done')) CHECK (state IN ('running', 'done', 'failed'))
);
```

```sql
SELECT state FROM jobs
-- columns: state: typing.Literal['running', 'done'] | None (TEXT)
```

## Conditions that do not list values give none

`schema.sql`:

```sql
CREATE TABLE t (
    id INTEGER PRIMARY KEY,
    a TEXT CHECK (a <> ''),
    b TEXT CHECK (a IN ('x') OR length(b) > 3),
    c TEXT CHECK (c NOT IN ('x')),
    d TEXT CHECK (d IN ('a', 'b') OR d LIKE 'z%'),
    e INTEGER CHECK (e IN (1, 2)) NOT NULL
);
```

```sql
SELECT a, b, c, d, e FROM t
-- columns: a: str | None (TEXT)
-- columns: b: str | None (TEXT)
-- columns: c: str | None (TEXT)
-- columns: d: str | None (TEXT)
-- columns: e: typing.Literal[1, 2] (INT)
```

## Types that cannot hold the values

A `CHAR(n)` column returns its values padded, so it keeps its type, and so does a column
whose values are of another kind than its type.

`schema.sql`:

```sql
CREATE TABLE t (
    id INTEGER PRIMARY KEY,
    code CHAR(2) CHECK (code IN ('a', 'b')),
    n INTEGER CHECK (n IN ('1', '2')),
    s TEXT CHECK (s IN (1, 2))
);
```

```sql
SELECT code, n, s FROM t
-- columns: code: str | None (CHAR(2))
-- columns: n: int | None (INT)
-- columns: s: str | None (TEXT)
```

## More than 32 values

`schema.sql`:

```sql
CREATE TABLE big (
    id INTEGER PRIMARY KEY,
    code INTEGER CHECK (code IN (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33))
);
```

```sql
SELECT code FROM big
-- columns: code: int | None (INT)
```

## Domains

`schema.sql`:

```sql
CREATE DOMAIN status AS text CHECK (VALUE IN ('new', 'done'));
CREATE DOMAIN final_status AS status CHECK (VALUE <> 'new');
CREATE DOMAIN grade AS integer CHECK (VALUE = ANY (ARRAY[1, 2, 3]));
CREATE DOMAIN percent AS integer CHECK (VALUE BETWEEN 0 AND 100);
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    status status NOT NULL,
    grade grade,
    percent percent,
    history status[]
);
```

```sql
SELECT status, grade, percent FROM items
-- columns: status: typing.Literal['new', 'done'] (TEXT)
-- columns: grade: typing.Literal[1, 2, 3] | None (INT)
-- columns: percent: int | None (INT)
```

### Cast to a domain

A cast to a domain gives its values.

```sql
SELECT 'x'::status AS s, id::grade AS g FROM items
-- columns: s: typing.Literal['new', 'done'] (TEXT)
-- columns: g: typing.Literal[1, 2, 3] (INT)
```

## Domain over a domain

A domain over a domain intersects the values of both `CHECK`s that list them.

`schema.sql`:

```sql
CREATE DOMAIN status AS text CHECK (VALUE IN ('new', 'open', 'done'));
CREATE DOMAIN closed AS status CHECK (VALUE IN ('done', 'dropped'));
CREATE TABLE items (id INTEGER PRIMARY KEY, state closed NOT NULL);
```

```sql
SELECT state FROM items
-- columns: state: typing.Literal['done'] (TEXT)
```

## ALTER TABLE

`ADD CONSTRAINT ... CHECK` limits the column, unless it is `NOT VALID` (rows already
there may break it); changing the column type forgets the values.

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, b TEXT, d TEXT CHECK (d IN ('y')), e TEXT CHECK (e IN ('z')));
ALTER TABLE t ADD CONSTRAINT a_values CHECK (a IN ('p', 'q'));
ALTER TABLE t ADD CONSTRAINT b_values CHECK (b IN ('p', 'q')) NOT VALID;
ALTER TABLE t ALTER COLUMN d TYPE varchar(5);
```

```sql
SELECT a, b, d, e FROM t
-- columns: a: typing.Literal['p', 'q'] | None (TEXT)
-- columns: b: str | None (TEXT)
-- columns: d: str | None (VARCHAR(5))
-- columns: e: typing.Literal['z'] | None (TEXT)
```

## DROP CONSTRAINT

Constraints are not followed by name, so dropping one forgets the values of the table's
columns.

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER PRIMARY KEY, c TEXT CHECK (c IN ('x')), e TEXT CHECK (e IN ('z')));
ALTER TABLE t DROP CONSTRAINT t_c_check;
```

```sql
SELECT c, e FROM t
-- columns: c: str | None (TEXT)
-- columns: e: str | None (TEXT)
```

## ALTER DOMAIN

An `ALTER DOMAIN` that changes a constraint leaves the domain's Python type unknown.

`schema.sql`:

```sql
CREATE DOMAIN status AS text CHECK (VALUE IN ('new', 'done'));
CREATE TABLE items (id INTEGER PRIMARY KEY, status status NOT NULL);
ALTER DOMAIN status DROP CONSTRAINT status_check;
```

```sql
SELECT status FROM items
-- columns: status: None (STATUS)
```

## Views, CTEs and subqueries carry the values

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
CREATE VIEW order_status AS SELECT id, status, 'order' AS kind FROM orders;
```

```sql
SELECT status, kind FROM order_status
-- columns: status: typing.Literal['pending', 'paid'] (TEXT)
-- columns: kind: typing.Literal['order'] (None)
```

```sql
WITH o AS (SELECT status FROM orders) SELECT status FROM (SELECT status FROM o) t
-- columns: status: typing.Literal['pending', 'paid'] (TEXT)
```

## UNION, CASE and COALESCE with a column of values

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
CREATE TABLE refunds (id INTEGER PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('paid', 'refunded')));
```

```sql
SELECT status FROM orders UNION SELECT status FROM refunds
-- columns: status: typing.Literal['pending', 'paid', 'refunded'] (TEXT)
```

A constant joins the values; a column of unknown values widens them.

```sql
SELECT status FROM orders UNION ALL SELECT 'cancelled'
-- columns: status: typing.Literal['pending', 'paid', 'cancelled'] (TEXT)
```

```sql
SELECT status FROM orders UNION ALL SELECT name::TEXT FROM (SELECT 'x' AS name) t
-- columns: status: str (TEXT)
```

```sql
SELECT COALESCE(status, 'none') AS a, CASE WHEN id > 1 THEN status ELSE 'first' END AS b,
       CASE WHEN id > 1 THEN status ELSE name END AS c
FROM (SELECT o.id, o.status, 'n'::TEXT AS name FROM orders o) t
-- columns: a: typing.Literal['pending', 'paid', 'none'] (TEXT)
-- columns: b: typing.Literal['pending', 'paid', 'first'] (TEXT)
-- columns: c: str (TEXT)
```

A function of the column gives the plain type.

```sql
SELECT upper(status) AS up, status || '!' AS bang, lower(status)::TEXT AS low, MAX(status) AS biggest
FROM orders
-- columns: up: str (TEXT)
-- columns: bang: str (TEXT)
-- columns: low: str (TEXT)
-- columns: biggest: typing.Literal['pending', 'paid'] | None (TEXT)
```

## Enums

An enum keeps all its labels in a union with a constant or another value set of the same
enum.

`schema.sql`:

```sql
CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');
CREATE TABLE people (id INTEGER PRIMARY KEY, mood mood NOT NULL, other mood);
```

```sql
SELECT mood FROM people UNION SELECT other FROM people
-- columns: mood: typing.Literal['sad', 'ok', 'happy'] | None (MOOD)
```

```sql
SELECT mood, CASE WHEN id > 1 THEN mood ELSE 'sad' END AS m FROM people
-- columns: mood: typing.Literal['sad', 'ok', 'happy'] (MOOD)
-- columns: m: typing.Literal['sad', 'ok', 'happy'] (MOOD)
```

## Rewrite

```python
q = "--sql\nSELECT status, priority, note FROM orders"
# rewritten: q = SQL[tuple[typing.Literal['pending', 'paid', 'refunded'], typing.Literal[1, 2, 3] | None, str | None]]("--sql\nSELECT status, priority, note FROM orders")
```

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded')),
    priority INTEGER CHECK (priority IN (1, 2, 3)),
    note TEXT
);
```

## Collations and citext

A collation may equate different strings (a nondeterministic one, `citext`), so the values a
`CHECK` lists do not bound the stored ones. `"C"` and `POSIX` are exact.

`schema.sql`:

```sql
CREATE DOMAIN loose AS text COLLATE "und-x-icu" CHECK (VALUE IN ('a', 'b'));
CREATE DOMAIN exact AS text COLLATE "C" CHECK (VALUE IN ('a', 'b'));
CREATE DOMAIN folded AS citext CHECK (VALUE IN ('a', 'b'));
CREATE DOMAIN tight AS text CHECK (VALUE IN ('a', 'b'));
CREATE TABLE t (
    id INTEGER PRIMARY KEY,
    a TEXT COLLATE "und-x-icu" CHECK (a IN ('a', 'b')),
    b TEXT COLLATE "C" CHECK (b IN ('a', 'b')),
    c loose,
    d exact,
    e folded,
    f tight COLLATE "und-x-icu",
    g tight
);
```

```sql
SELECT a, b, c, d, e, f, g FROM t
-- columns: a: str | None (TEXT)
-- columns: b: typing.Literal['a', 'b'] | None (TEXT)
-- columns: c: str | None (TEXT)
-- columns: d: str | None (TEXT)
-- columns: e: str | None (CITEXT)
-- columns: f: str | None (TEXT)
-- columns: g: typing.Literal['a', 'b'] | None (TEXT)
```

## Repeated values

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT CHECK (a IN ('x', 'y', 'x')), b INTEGER CHECK (b = ANY (ARRAY[1, 1, 2])));
```

```sql
SELECT a, b FROM t
-- columns: a: typing.Literal['x', 'y'] | None (TEXT)
-- columns: b: typing.Literal[1, 2] | None (INT)
```

## Views over a table whose constraint is dropped

A view typed while the `CHECK` held would keep its values, so dropping a constraint widens the
text and integer columns of the existing views. Enum labels and constants stay.

`schema.sql`:

```sql
CREATE TYPE mood AS ENUM ('sad', 'ok');
CREATE TABLE t (id INTEGER PRIMARY KEY, s TEXT CHECK (s IN ('a', 'b')), m mood);
CREATE VIEW v AS SELECT s, m, 'k' AS kind FROM t;
ALTER TABLE t DROP CONSTRAINT t_s_check;
CREATE VIEW w AS SELECT s FROM t;
```

```sql
SELECT s, m, kind FROM v
-- columns: s: str | None (TEXT)
-- columns: m: typing.Literal['sad', 'ok'] | None (MOOD)
-- columns: kind: typing.Literal['k'] (None)
```

```sql
SELECT s FROM w
-- columns: s: str | None (TEXT)
```
