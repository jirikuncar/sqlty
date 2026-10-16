# Conditions narrow value sets

A column with a value set (an enum, a `CHECK (... IN (...))`, a domain) keeps
only the values its conditions allow: `WHERE status = 'paid'` reads `status`
as `Literal['paid']`. `=` and `IN` narrow; `AND` keeps the values both sides
allow, `OR` those either side allows, and a `CASE` branch reads its column
where its `WHEN` holds.

`schema.sql`:

```sql
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('new', 'paid', 'shipped')),
    tag CITEXT CHECK (tag IN ('a', 'b'))
);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## WHERE equality and IN

```sql
SELECT status FROM orders WHERE status = 'paid'
-- inferred: tuple[typing.Literal['paid']]
```

```sql
SELECT status FROM orders WHERE status IN ('paid', 'shipped')
-- inferred: tuple[typing.Literal['paid', 'shipped']]
```

## AND and OR

```sql
SELECT status FROM orders WHERE status IN ('paid', 'shipped') AND status IN ('new', 'paid')
-- inferred: tuple[typing.Literal['paid']]
```

```sql
SELECT status FROM orders WHERE status = 'new' OR status = 'paid'
-- inferred: tuple[typing.Literal['new', 'paid']]
```

An `OR` side that says nothing about the column keeps every value.

```sql
SELECT status FROM orders WHERE status = 'new' OR id = 1
-- inferred: tuple[typing.Literal['new', 'paid', 'shipped']]
```

## Joins and CASE

Each relation keeps its own condition.

```sql
SELECT a.status, b.status FROM orders a JOIN orders b ON b.status = 'new' WHERE a.status = 'paid'
-- inferred: tuple[typing.Literal['paid'], typing.Literal['new']]
```

```sql
SELECT CASE WHEN status = 'paid' THEN status ELSE 'other' END AS s FROM orders
-- inferred: tuple[typing.Literal['paid', 'other']]
```

## What does not narrow

A negated condition keeps every value, and so does a condition no row can meet.

```sql
SELECT status FROM orders WHERE NOT status = 'paid'
-- inferred: tuple[typing.Literal['new', 'paid', 'shipped']]
```

```sql
SELECT status FROM orders WHERE status = 'lost'
-- inferred: tuple[typing.Literal['new', 'paid', 'shipped']]
```

## Case-insensitive comparisons

### citext

A `citext` column compares without case, so `tag = 'A'` also matches `'a'`: it
is not narrowed.

```sql
SELECT tag FROM orders WHERE tag = 'a'
-- inferred: tuple[str]
```

### MySQL

MySQL compares strings with a case-insensitive collation by default, so it
never narrows.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INT PRIMARY KEY, status ENUM('new', 'paid') NOT NULL);
```

```sql
SELECT status FROM orders WHERE status = 'paid'
-- inferred: tuple[typing.Literal['new', 'paid']]
```
