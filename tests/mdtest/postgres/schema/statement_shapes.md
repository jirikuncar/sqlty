# How the schema loader reads statements

Statements the loader reads by their words before a parser sees them: domains, constraints
sqlparser cannot hold, `SET search_path`, `LIKE`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Domains name a base type and their checks

A domain's base type types its columns, in any spelling of the name; `CHECK (VALUE IN
(...))` limits the values, however the checks are spelled.

`schema.sql`:

```sql
CREATE DOMAIN app."p" AS numeric(5, 2) NOT NULL DEFAULT 0;
CREATE DOMAIN d text COLLATE "C" CHECK (VALUE <> '');
create domain a.b as numeric(5, 2) not null default 0;
CREATE DOMAIN choice text CHECK (VALUE IN ('a', 'b)')) CONSTRAINT c CHECK ((VALUE)::text <> 'x');
CREATE DOMAIN plain text NOT NULL;
CREATE DOMAIN nothing;
CREATE TABLE t (p app."p", q d, r a.b, s choice, u plain);
```

```sql
SELECT * FROM t
-- columns: p: decimal.Decimal | None (DECIMAL(5, 2))
-- columns: q: str | None (TEXT)
-- columns: r: decimal.Decimal | None (DECIMAL(5, 2))
-- columns: s: typing.Literal['a', 'b)'] | None (TEXT)
-- columns: u: str | None (TEXT)
```

## Constraints are read without their text

A column keeps its type and `NOT NULL` whatever constraints follow it, and a table
without any is read as written.

`schema.sql`:

```sql
CREATE TABLE t (a INT CHECK (a > 0) NOT NULL, b INT REFERENCES u(id) ON DELETE SET NULL (b) NOT NULL, c TEXT, CONSTRAINT k CHECK (length(c) > 0), PRIMARY KEY (a));
CREATE TABLE plain (a INT, b TEXT);
CREATE TABLE u (a INT CONSTRAINT c REFERENCES public.t (a) MATCH FULL ON UPDATE CASCADE DEFERRABLE INITIALLY DEFERRED DEFAULT 1);
CREATE TABLE odd (a INT, , CHECK (a > 0));
CREATE TABLE odd2 (a INT REFERENCES u (a) NOT DEFERRABLE);
```

```sql
SELECT * FROM t
-- columns: a: int (INT)
-- columns: b: int (INT)
-- columns: c: str | None (TEXT)
```

```sql
SELECT * FROM plain
-- columns: a: int | None (INT)
-- columns: b: str | None (TEXT)
```

```sql
SELECT * FROM u
-- columns: a: int | None (INT)
```

```sql
SELECT * FROM odd
-- columns: a: int | None (INT)
```

```sql
SELECT * FROM odd2
-- columns: a: int | None (INT)
```

## search_path in its spellings

`schema.sql`:

```sql
SET search_path TO app, "$user", Public;
CREATE TABLE one (id int);
SET LOCAL search_path = 'a';
CREATE TABLE two (id int);
SET search_path TO DEFAULT;
CREATE TABLE three (id int);
SELECT pg_catalog.set_config('search_path', 'b, c', false);
CREATE TABLE four (id int);
SELECT pg_catalog.set_config('search_path', '', false);
SET statement_timeout = 0;
SET search_path FROM CURRENT;
```

```sql
SELECT * FROM app.one
-- columns: id: int | None (INT)
```

```sql
SELECT * FROM a.two
-- columns: id: int | None (INT)
```

```sql
SELECT * FROM public.three
-- columns: id: int | None (INT)
```

```sql
SELECT * FROM b.four
-- columns: id: int | None (INT)
```

## LIKE copies columns

The copied columns go where `LIKE` stands in the column list, as PostgreSQL builds the
table from the list's items in order
([CREATE TABLE](https://www.postgresql.org/docs/17/sql-createtable.html), `LIKE
source_table`).

`schema.sql`:

```sql
CREATE TABLE copy (LIKE app.a INCLUDING ALL, x int, LIKE b);
CREATE TABLE app.a (k int NOT NULL);
CREATE TABLE b (y text);
CREATE TABLE p PARTITION OF q DEFAULT;
CREATE TABLE q (id int) PARTITION BY LIST (id);
```

```sql
SELECT * FROM copy
-- columns: k: int (INT)
-- columns: x: int | None (INT)
-- columns: y: str | None (TEXT)
```

```sql
SELECT * FROM p
-- columns: id: int | None (INT)
```

## Views and tables named by their words

`schema.sql`:

```sql
CREATE OR REPLACE MATERIALIZED VIEW mv AS SELECT 1 AS x;
CREATE TABLE view (a INT);
```

```sql
SELECT * FROM mv
-- columns: x: int (None)
```

```sql
SELECT * FROM view
-- columns: a: int | None (INT)
```

## Column constraints the loader drops when the table does not parse

A table constraint sqlparser cannot read makes the loader drop every constraint and column
`CHECK` and `REFERENCES` clause (whatever follows it: schema-qualified targets, `MATCH`,
`ON DELETE` and `ON UPDATE` actions, `DEFERRABLE`, `INITIALLY`) before reading the columns.

`schema.sql`:

```sql
CREATE TABLE s1 (
    a INT CONSTRAINT c REFERENCES public.u (id) MATCH FULL ON DELETE SET NULL ON UPDATE NO ACTION DEFERRABLE INITIALLY DEFERRED DEFAULT 1,
    b INT REFERENCES u NOT DEFERRABLE NOT NULL,
    c INT CHECK (c > 0) NOT NULL,
    d INT CONSTRAINT k CHECK (d > 0),
    e INT REFERENCES app.u ON DELETE CASCADE ON UPDATE SET DEFAULT,
    f INT REFERENCES u (id) ON DELETE SET NULL (f),
    g INT CONSTRAINT r REFERENCES u,
    CONSTRAINT bad NONSENSE GARBAGE (a)
);
```

```sql
SELECT * FROM s1
-- columns: a: int | None (INT)
-- columns: b: int (INT)
-- columns: c: int (INT)
-- columns: d: int | None (INT)
-- columns: e: int | None (INT)
-- columns: f: int | None (INT)
-- columns: g: int | None (INT)
```
