# Domains and enum types

A column typed by a domain gets the domain's base type; one typed by an enum is
a `typing.Literal` of the enum's labels. The `-- columns:` SQL type is the base
type for a domain and the enum's own (unqualified, upper-case) name for an enum.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Domain resolves to its base type

`schema.sql`:

```sql
CREATE DOMAIN percent AS integer CHECK (VALUE BETWEEN 0 AND 100);
CREATE TABLE orders (id INTEGER PRIMARY KEY, discount percent);
```

```sql
SELECT discount FROM orders
-- inferred: tuple[int | None]
-- columns: discount: int | None (INT)
```

## Domains over domains and qualified names

The definition may come after the domain that builds on it, and a schema-qualified
name matches the unqualified one and the other way round.

`schema.sql`:

```sql
SET search_path TO app, public;
CREATE DOMAIN app.small_percent AS percent NOT NULL DEFAULT 0;
CREATE DOMAIN app.percent AS numeric(5, 2);
CREATE DOMAIN label AS varchar(20);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    discount small_percent,
    rate app.percent,
    note app.label
);
```

```sql
SELECT discount, rate, note FROM orders
-- columns: discount: decimal.Decimal | None (DECIMAL(5, 2))
-- columns: rate: decimal.Decimal | None (DECIMAL(5, 2))
-- columns: note: str | None (VARCHAR(20))
```

## Enum resolves to Literal

`schema.sql`:

```sql
CREATE TYPE order_status AS ENUM ('pending', 'paid');
CREATE TABLE orders (id INTEGER PRIMARY KEY, status order_status NOT NULL);
```

```sql
SELECT status FROM orders
-- inferred: tuple[typing.Literal['pending', 'paid']]
-- columns: status: typing.Literal['pending', 'paid'] (ORDER_STATUS)
```

## Enum labels with quotes, a domain over an enum, and a cast

`schema.sql`:

```sql
SET search_path TO app, public;
CREATE TYPE app.mood AS ENUM ('it''s fine', 'bad');
CREATE DOMAIN current_mood AS mood;
CREATE TABLE people (id INTEGER PRIMARY KEY, mood current_mood, name TEXT);
```

```sql
SELECT mood, name::mood, name::app.mood, id FROM people
-- inferred: tuple[typing.Literal['it\'s fine', 'bad'] | None, typing.Literal['it\'s fine', 'bad'] | None, typing.Literal['it\'s fine', 'bad'] | None, int]
```

## Views keep enum and domain columns

`schema.sql`:

```sql
CREATE TYPE order_status AS ENUM ('pending', 'paid');
CREATE DOMAIN percent AS integer;
CREATE TABLE orders (id INTEGER PRIMARY KEY, status order_status, discount percent);
CREATE VIEW open_orders AS SELECT status, discount FROM orders WHERE status = 'pending';
```

```sql
SELECT status, discount FROM open_orders
-- inferred: tuple[typing.Literal['pending'], int | None]
```

## Arrays of enums and domains

The element type resolves through the enum or domain, also after a subscript.

`schema.sql`:

```sql
CREATE TYPE order_status AS ENUM ('pending', 'paid');
CREATE DOMAIN short_text AS varchar(20);
CREATE TABLE orders (id INTEGER PRIMARY KEY, status order_status NOT NULL, history order_status[] NOT NULL, labels short_text[]);
```

```sql
SELECT history, labels FROM orders
-- inferred: tuple[list[typing.Literal['pending', 'paid']], list[str] | None]
```

```sql
SELECT history[1], labels[1] FROM orders
-- inferred: tuple[typing.Literal['pending', 'paid'] | None, str | None]
```

An array a query builds of enum values lists the labels too:

```sql
SELECT ARRAY[status] AS a, ARRAY(SELECT status FROM orders) AS b, array_agg(status) AS c FROM orders GROUP BY status
-- columns: a: list[typing.Literal['pending', 'paid']] (ARRAY<ORDER_STATUS>)
-- columns: b: list[typing.Literal['pending', 'paid']] (ARRAY<ORDER_STATUS>)
-- columns: c: list[typing.Literal['pending', 'paid']] (ARRAY<ORDER_STATUS>)
```

## Enum support functions

`enum_first` and `enum_last` give the first and last label of the argument's
type, and `enum_range` its labels in order. They read only the type of the
argument, so a NULL still gives a value.

`schema.sql`:

```sql
CREATE TYPE order_status AS ENUM ('pending', 'paid', 'shipped');
CREATE TABLE orders (id INTEGER PRIMARY KEY, status order_status);
```

```sql
SELECT enum_first(status) AS a, enum_last(NULL::order_status) AS b, enum_range(status) AS c, enum_range('paid'::order_status, NULL) AS d FROM orders
-- columns: a: typing.Literal['pending', 'paid', 'shipped'] (ORDER_STATUS)
-- columns: b: typing.Literal['pending', 'paid', 'shipped'] (ORDER_STATUS)
-- columns: c: list[typing.Literal['pending', 'paid', 'shipped']] (ARRAY<ORDER_STATUS>)
-- columns: d: list[typing.Literal['pending', 'paid', 'shipped']] (ARRAY<ORDER_STATUS>)
-- rules: a: postgres.enum.bound / postgres.enum.bound
-- rules: b: postgres.enum.bound / postgres.enum.bound
-- rules: c: postgres.enum.range / postgres.enum.range
-- rules: d: postgres.enum.range / postgres.enum.range
```

## Rewrite imports typing

```python
q = "--sql\nSELECT status FROM orders"
# rewritten: q = SQL[tuple[typing.Literal['pending', 'paid']]]("--sql\nSELECT status FROM orders")
```

`schema.sql`:

```sql
CREATE TYPE order_status AS ENUM ('pending', 'paid');
CREATE TABLE orders (id INTEGER PRIMARY KEY, status order_status NOT NULL);
```
