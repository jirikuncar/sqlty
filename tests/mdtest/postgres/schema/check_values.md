# CHECK constraints that limit a column's values

A `CHECK` that lists the values of a text column makes its type a `Literal`; one the
loader cannot read leaves `str`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## IN limits the column to the listed strings

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status IN ('a', 'b')));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## = ANY of a cast array limits the column to its elements

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK ((status)::text = ANY ((ARRAY['a'::text, 'b'::text])::text[])));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## Repeated values in an IN list count once

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status IN ('a', 'b', 'a')));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## Repeated values in an ANY array count once

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = ANY (ARRAY['a', 'a'])));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a'] | None (TEXT)
```

## Or null arm

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = 'a' OR status = 'b' OR status IS NULL));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## Or unknown arm

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = 'a' OR length(status) > 3));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```

## And keeps known

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status IN ('a', 'b', 'c') AND status <> ''));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b', 'c'] | None (TEXT)
```

## And intersects

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status IN ('a', 'b') AND status IN ('b', 'c')));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['b'] | None (TEXT)
```

## NOT IN leaves the column a plain str

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status NOT IN ('a')));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```

## A CAST to boolean around a comparison still limits the column

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (CAST(status = 'a' AS boolean)));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a'] | None (TEXT)
```

## A table-qualified column on the right of = limits the column

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK ('a' = t.status));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a'] | None (TEXT)
```

## Or parenthesized null

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = 'a' OR (status IS NULL)));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a'] | None (TEXT)
```

## = ANY of a placeholder leaves the column a plain str

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = ANY ($1)));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```

## Comparing with a negated string leaves the column a plain str

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = -'a'));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```
