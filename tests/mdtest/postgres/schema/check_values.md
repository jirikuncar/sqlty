# CHECK constraints that limit a column's values

A `CHECK` that lists the values of a text column makes its type a `Literal`; one the
loader cannot read leaves `str`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## In list

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status IN ('a', 'b')));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## Any array

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK ((status)::text = ANY ((ARRAY['a'::text, 'b'::text])::text[])));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## In list duplicates

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status IN ('a', 'b', 'a')));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a', 'b'] | None (TEXT)
```

## Any array duplicates

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

## Not in

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status NOT IN ('a')));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```

## Cast boolean

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (CAST(status = 'a' AS boolean)));
```

```sql
SELECT status FROM t
-- columns: status: typing.Literal['a'] | None (TEXT)
```

## Qualified

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

## Any placeholder

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = ANY ($1)));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```

## Negated literal

`schema.sql`:

```sql
CREATE TABLE t (status text CHECK (status = -'a'));
```

```sql
SELECT status FROM t
-- columns: status: str | None (TEXT)
```
