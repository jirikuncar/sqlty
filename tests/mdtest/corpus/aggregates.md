# Corpus: aggregates

Every aggregate except `count` is NULL over zero input rows, which an ungrouped query or a
`FILTER` can give. Array aggregates keep NULL inputs as NULL elements.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## array_agg over a LEFT JOIN

<!-- mdtest:xfail array elements are typed non-null, list[str] instead of list[str | None] -->

Every group has a row, so the array is not NULL, but a book without authors aggregates a
NULL name into `{NULL}`.

`schema.sql`:

```sql
CREATE TABLE books (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL);
CREATE TABLE authors (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE book_authors (
    book_id BIGINT NOT NULL REFERENCES books(id),
    author_id BIGINT NOT NULL REFERENCES authors(id)
);
```

```sql
SELECT b.id, b.title, array_agg(a.name)::TEXT[] AS author_names
FROM books b
LEFT JOIN book_authors ba ON ba.book_id = b.id
LEFT JOIN authors a ON a.id = ba.author_id
GROUP BY b.id, b.title
-- inferred: tuple[int, str, list[str | None]]
```

## string_agg of text

`string_agg(text, text)` returns text, NULL over no rows.

`schema.sql`:

```sql
CREATE TABLE authors (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
```

```sql
SELECT string_agg(name, ', ' ORDER BY name) AS names FROM authors
-- inferred: tuple[str | None]
```

## sum of double precision

`sum(double precision)` is double precision; `FLOAT` without a precision is double precision.

`schema.sql`:

```sql
CREATE TABLE segments (length FLOAT NOT NULL);
```

```sql
SELECT SUM(length) AS total FROM segments
-- inferred: tuple[float | None]
```

## sum of numeric with an array exclusion

`sum(numeric)` is numeric, NULL when no row passes the filter; `<> ALL ($1)` takes a text
array.

`schema.sql`:

```sql
CREATE TABLE ticket (id INTEGER PRIMARY KEY, price NUMERIC(10, 2) NOT NULL, status TEXT NOT NULL);
```

```sql
SELECT SUM(price) AS presale_orders_platform_fee FROM ticket t WHERE t.status <> ALL ($1)
-- inferred: tuple[decimal.Decimal | None]
-- params: 1: list[str] (ARRAY<TEXT>)
```

## array_agg with DISTINCT and FILTER in a CTE

Without `GROUP BY`, either aggregate can see no rows and return NULL.

`schema.sql`:

```sql
CREATE TABLE my_table (col INTEGER NOT NULL);
```

```sql
WITH x AS (
    SELECT array_agg(DISTINCT my_table.col) AS col1,
           array_agg(DISTINCT my_table.col) FILTER (WHERE my_table.col > 10) AS col2
    FROM my_table
)
SELECT x.col1, x.col2 FROM x
-- inferred: tuple[list[int] | None, list[int] | None]
```
