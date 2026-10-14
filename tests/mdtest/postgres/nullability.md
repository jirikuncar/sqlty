# Nullability of expressions (PostgreSQL)

An expression that can be NULL infers `T | None`. Expressions
sqlty cannot read stay non-optional.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    nick TEXT,
    age INTEGER,
    score NUMERIC(10, 2),
    prefs JSONB NOT NULL,
    tags TEXT[] NOT NULL
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total NUMERIC(10, 2) NOT NULL,
    note TEXT
);
```

## Strict functions and operators

A function or operator is NULL when one of its arguments is.

```sql
SELECT lower(name), lower(nick), age + 1, id + 1, name || nick FROM users
-- inferred: tuple[str, str | None, int | None, int, str | None]
```

Comparisons and `IS [NOT] NULL` tests:

```sql
SELECT age > 18, id > 18, age IS NULL, nick IS NOT NULL, nick IS DISTINCT FROM 'x' FROM users
-- inferred: tuple[bool | None, bool, bool, bool, bool]
```

`IN` is NULL when the left side is, or when the list holds a NULL.

```sql
SELECT age IN (1, 2), id IN (1, 2), id IN (1, NULL) FROM users
-- inferred: tuple[bool | None, bool, bool | None]
```

## COALESCE, NULLIF and friends

`COALESCE` is NULL only when every argument can be.

```sql
SELECT coalesce(nick, name), coalesce(nick, age::TEXT), coalesce(nick, NULL) FROM users
-- inferred: tuple[str, str | None, str | None]
```

`NULLIF` can always return NULL.

```sql
SELECT nullif(id, 0), nullif(name, '') FROM users
-- inferred: tuple[int | None, str | None]
```

`GREATEST` and `LEAST` skip NULLs in PostgreSQL.

```sql
SELECT greatest(age, 18), greatest(age, nick::INT) FROM users
-- inferred: tuple[int, int | None]
```

`concat` skips NULLs; `concat_ws` is NULL only for a NULL separator.

```sql
SELECT concat(name, nick), concat_ws(',', name, nick) FROM users
-- inferred: tuple[str, str]
```

## Always-nullable functions and accessors

```sql
SELECT prefs -> 'theme', prefs ->> 'theme', prefs #>> '{a,b}' FROM users
-- inferred: tuple[typing.Any, str | None, str | None]
```

The JSON column is `Any`, which already admits `None`, so only the text accessors are
spelled optional.

```sql
SELECT tags[1], array_length(tags, 1) FROM users
-- inferred: tuple[str | None, int | None]
```

## CASE

A `CASE` without `ELSE` is NULL when no branch matches.

```sql
SELECT
    CASE WHEN age > 18 THEN name END,
    CASE WHEN age > 18 THEN name ELSE 'minor' END,
    CASE WHEN age > 18 THEN nick ELSE 'minor' END
FROM users
-- inferred: tuple[str | None, str, str | None]
```

## Aggregates

Over an empty input an aggregate other than `COUNT` is NULL.

```sql
SELECT count(*), count(nick), max(id), sum(age), avg(id) FROM users
-- inferred: tuple[int, int, int | None, int | None, decimal.Decimal | None]
```

Every group of a `GROUP BY` has a row, so only a nullable argument gives NULL.

```sql
SELECT name, count(*), max(id), max(age), min(score) FROM users GROUP BY name
-- inferred: tuple[str, int, int, int | None, decimal.Decimal | None]
```

A `FILTER` can leave a group empty.

```sql
SELECT name, max(id) FILTER (WHERE age > 18) FROM users GROUP BY name
-- inferred: tuple[str, int | None]
```

`array_agg` over a group of rows is an array, even of NULLs.

```sql
SELECT name, array_agg(id), array_agg(age) FROM users GROUP BY name
-- inferred: tuple[str, list[int], list[int]]
```

Without `GROUP BY`, `array_agg` of no rows is NULL.

```sql
SELECT array_agg(id) FROM users
-- inferred: tuple[list[int] | None]
```

## Window functions

```sql
SELECT
    row_number() OVER (ORDER BY id),
    count(age) OVER (),
    sum(id) OVER (ORDER BY id),
    lag(id) OVER (ORDER BY id),
    lag(id, 1, 0) OVER (ORDER BY id),
    first_value(age) OVER (ORDER BY id)
FROM users
-- inferred: tuple[int, int, int, int | None, int, int | None]
```

## Scalar subqueries

A scalar subquery with no row reads as NULL, so it is optional unless it always
returns one row.

```sql
SELECT (SELECT max(total) FROM orders), (SELECT count(*) FROM orders), (SELECT 1) FROM users
-- inferred: tuple[decimal.Decimal | None, int, int]
```

```sql
SELECT (SELECT note FROM orders o WHERE o.user_id = u.id LIMIT 1) FROM users u
-- inferred: tuple[str | None]
```

```sql
SELECT (SELECT total FROM orders o WHERE o.user_id = u.id ORDER BY id LIMIT 1) FROM users u
-- inferred: tuple[decimal.Decimal | None]
```

`IN (subquery)` is NULL when its column can be.

```sql
SELECT id IN (SELECT user_id FROM orders), id IN (SELECT id FROM orders WHERE note IS NULL) FROM users
-- inferred: tuple[bool, bool]
```

```sql
SELECT age IN (SELECT user_id FROM orders), id IN (SELECT length(note) FROM orders) FROM users
-- inferred: tuple[bool | None, bool | None]
```

## Unknown nullability stays non-optional

A placeholder cast has no schema column behind it, so it is not optional. A function sqlty
does not know leaves the whole row type unknown.

```sql
SELECT id::TEXT, $1::INT, age::TEXT FROM users
-- inferred: tuple[str, int, str | None]
```
