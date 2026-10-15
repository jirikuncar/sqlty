# Nullability: narrowing by conditions (PostgreSQL)

A row that survives a condition NULL cannot satisfy has a non-null column there.
`WHERE`, `HAVING` and the `ON` of inner joins narrow the columns they
test.

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
    prefs JSONB NOT NULL
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total NUMERIC(10, 2),
    note TEXT
);
```

## Without a condition

```sql
SELECT nick, age FROM users
-- inferred: tuple[str | None, int | None]
```

## IS NOT NULL

```sql
SELECT nick, age FROM users WHERE nick IS NOT NULL
-- inferred: tuple[str, int | None]
```

`SELECT *` is narrowed too.

```sql
SELECT * FROM users WHERE age IS NOT NULL
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: nick: str | None (TEXT)
-- columns: age: int (INT)
-- columns: prefs: typing.Any (JSONB)
```

## Comparisons, IN, BETWEEN and LIKE

NULL is never equal, less or greater than anything, nor `IN` a list, `BETWEEN` bounds or
`LIKE` a pattern, so a row that passes has the column.

```sql
SELECT age FROM users WHERE age = 18
-- inferred: tuple[int]
```

```sql
SELECT age, nick FROM users WHERE age > 18 AND nick LIKE 'a%'
-- inferred: tuple[int, str]
```

```sql
SELECT age FROM users WHERE age IN (18, 21)
-- inferred: tuple[int]
```

```sql
SELECT age FROM users WHERE age BETWEEN 18 AND 65
-- inferred: tuple[int]
```

Through a strict function or arithmetic: `lower(nick)` is NULL only for a NULL `nick`.

```sql
SELECT nick FROM users WHERE lower(nick) = 'x'
-- inferred: tuple[str]
```

```sql
SELECT age FROM users WHERE age + 1 > 19
-- inferred: tuple[int]
```

```sql
SELECT lower(nick) FROM users WHERE lower(nick) IS NOT NULL
-- inferred: tuple[str]
```

## Conditions that prove nothing

`OR` proves only what both sides prove.

```sql
SELECT age, nick FROM users WHERE age > 18 OR nick = 'x'
-- inferred: tuple[int | None, str | None]
```

```sql
SELECT age FROM users WHERE age > 18 OR age < 10
-- inferred: tuple[int]
```

`IS NULL`, `IS DISTINCT FROM` and `COALESCE(...)` tell nothing about the column.

```sql
SELECT age, nick FROM users WHERE age IS NULL AND coalesce(nick, 'x') = 'x'
-- inferred: tuple[int | None, str | None]
```

`NOT` flips what is proven: a false comparison is not NULL either.

```sql
SELECT age, nick FROM users WHERE NOT (age > 18) AND nick IS NOT NULL
-- inferred: tuple[int, str]
```

## HAVING

```sql
SELECT age, max(total) FROM users JOIN orders ON orders.user_id = users.id GROUP BY age HAVING age IS NOT NULL AND max(total) IS NOT NULL
-- inferred: tuple[int, decimal.Decimal]
```

## A JSON key test does not narrow the accessor

`doc ? 'key'` is also true for an array that holds the string `'key'`, and `doc -> 'key'` is
NULL there, so the accessor stays optional. The column itself is narrowed.

`typing.Any` already admits `None`, so a column check uses a cast to see it:

```sql
SELECT (prefs -> 'theme')::TEXT, (prefs ->> 'theme')::TEXT, (prefs -> 'lang')::TEXT FROM users WHERE prefs ? 'theme'
-- inferred: tuple[str | None, str | None, str | None]
```

Testing the accessor itself narrows it:

```sql
SELECT (prefs -> 'theme')::TEXT FROM users WHERE (prefs -> 'theme') IS NOT NULL
-- inferred: tuple[str]
```

## Outer joins: a non-null column proves the row is there

A `LEFT JOIN` makes the joined relation optional. A `WHERE` that NULL cannot satisfy on one
of its columns drops the null-extended rows, so the whole relation is back to its schema.

```sql
SELECT users.id, orders.id, orders.total FROM users LEFT JOIN orders ON orders.user_id = users.id
-- inferred: tuple[int, int | None, decimal.Decimal | None]
```

```sql
SELECT users.id, orders.id, orders.total FROM users LEFT JOIN orders ON orders.user_id = users.id WHERE orders.id IS NOT NULL
-- inferred: tuple[int, int, decimal.Decimal | None]
```

```sql
SELECT users.id, orders.id FROM users LEFT JOIN orders ON orders.user_id = users.id WHERE orders.total > 100
-- inferred: tuple[int, int]
```

`IS NULL` is the anti-join: the relation stays optional.

```sql
SELECT users.id, orders.id FROM users LEFT JOIN orders ON orders.user_id = users.id WHERE orders.id IS NULL
-- inferred: tuple[int, int | None]
```

An `OR` keeps the join optional unless both sides prove the relation.

```sql
SELECT users.id, orders.id FROM users LEFT JOIN orders ON orders.user_id = users.id WHERE users.id = 1 OR orders.id = 1
-- inferred: tuple[int, int | None]
```

```sql
SELECT users.id, orders.id FROM users LEFT JOIN orders ON orders.user_id = users.id WHERE orders.id = 1 OR orders.total > 1
-- inferred: tuple[int, int]
```

`RIGHT` and `FULL` joins null-extend the other side, and a condition on the preserved
side proves nothing about it.

```sql
SELECT users.id, orders.id FROM users RIGHT JOIN orders ON orders.user_id = users.id WHERE users.id = 1
-- inferred: tuple[int, int]
```

```sql
SELECT users.id, orders.id FROM users FULL JOIN orders ON orders.user_id = users.id WHERE users.id = 1
-- inferred: tuple[int, int | None]
```

### The ON of an outer join filters nothing

```sql
SELECT users.id, orders.id FROM users LEFT JOIN orders ON orders.user_id = users.id AND orders.total IS NOT NULL
-- inferred: tuple[int, int | None]
```

### The ON of an inner join narrows earlier outer joins

```sql
SELECT users.id, orders.id, o2.id FROM users LEFT JOIN orders ON orders.user_id = users.id JOIN orders o2 ON o2.id = orders.id
-- inferred: tuple[int, int, int]
```

An inner join inside the group that a `LEFT JOIN` null-extends narrows only the group:

```sql
SELECT users.id, orders.id, o2.id FROM users LEFT JOIN (orders JOIN orders o2 ON o2.id = orders.id) ON orders.user_id = users.id
-- inferred: tuple[int, int | None, int | None]
```

## AND, OR and CASE read their right side under the left

```sql
SELECT age IS NOT NULL AND age > 18, age > 18 AND age IS NOT NULL, age IS NULL OR age > 18 FROM users
-- inferred: tuple[bool, bool | None, bool]
```

```sql
SELECT
    CASE WHEN age IS NOT NULL THEN age ELSE 0 END,
    CASE WHEN age IS NULL THEN 0 ELSE age END,
    CASE WHEN age > 18 THEN age END,
    CASE WHEN age IS NULL THEN age ELSE 0 END
FROM users
-- inferred: tuple[int, int, int | None, int | None]
```

A simple `CASE` tests equality with its operand.

```sql
SELECT CASE age WHEN 18 THEN age ELSE 0 END, CASE nick WHEN 'x' THEN nick END FROM users
-- inferred: tuple[int, str | None]
```

Later branches see the earlier conditions fail.

```sql
SELECT CASE WHEN age IS NULL THEN 0 WHEN nick IS NULL THEN 1 ELSE age + length(nick) END FROM users
-- inferred: tuple[int]
```

## Narrowing stays in its query

A derived table or CTE is narrowed by its own conditions, not by the outer query.

```sql
SELECT age FROM (SELECT age FROM users WHERE age IS NOT NULL) d
-- inferred: tuple[int]
```

```sql
SELECT d.age FROM (SELECT age FROM users) d WHERE d.age IS NOT NULL
-- inferred: tuple[int]
```

A subquery reads the facts of the query around it:

```sql
SELECT (SELECT users.age) FROM users WHERE users.age IS NOT NULL
-- inferred: tuple[int]
```

```sql
SELECT (SELECT users.age) FROM users
-- inferred: tuple[int | None]
```
