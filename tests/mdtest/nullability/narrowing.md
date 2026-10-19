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

The rule of the call decides: `upper` of a range is NULL for an infinite bound too, so
`upper(int4range(age, 100))` proves nothing about `age`.

```sql
SELECT age FROM users WHERE upper(int4range(age, 100)) IS NOT NULL
-- inferred: tuple[int | None]
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

`IS NULL`, `IS DISTINCT FROM` a value and `COALESCE(...)` tell nothing about the column.

```sql
SELECT age, nick FROM users WHERE age IS NULL AND coalesce(nick, 'x') = 'x'
-- inferred: tuple[int | None, str | None]
```

```sql
SELECT nick FROM users WHERE nick IS DISTINCT FROM 'x'
-- inferred: tuple[str | None]
```

`NOT` flips what is proven: a false comparison is not NULL either.

```sql
SELECT age, nick FROM users WHERE NOT (age > 18) AND nick IS NOT NULL
-- inferred: tuple[int, str]
```

## IS DISTINCT FROM NULL

`x IS DISTINCT FROM NULL` is `x IS NOT NULL`, and `x IS NOT DISTINCT FROM NULL` is
`x IS NULL`.

```sql
SELECT nick, age FROM users WHERE nick IS DISTINCT FROM NULL AND NOT (age IS NOT DISTINCT FROM NULL)
-- inferred: tuple[str, int]
```

## EXISTS

A correlated `EXISTS` holds when some row passes its `WHERE`, so what that `WHERE`
proves about the outer query holds too. `NOT EXISTS`, and an aggregate that returns a
row when none matches, prove nothing.

```sql
SELECT u.nick FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.note = u.nick)
-- inferred: tuple[str]
```

```sql
SELECT u.nick FROM users u WHERE NOT EXISTS (SELECT 1 FROM orders o WHERE o.note = u.nick)
-- inferred: tuple[str | None]
```

```sql
SELECT u.nick FROM users u WHERE EXISTS (SELECT count(*) FROM orders o WHERE o.note = u.nick)
-- inferred: tuple[str | None]
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

## Outer joins to one row are never null-extended

An aggregate without `GROUP BY` returns exactly one row, so `LEFT JOIN ... ON TRUE` to it
always matches.

```sql
SELECT c.n, c.total FROM users
LEFT JOIN LATERAL (SELECT count(*) AS n, sum(orders.total) AS total FROM orders WHERE orders.user_id = users.id) c ON TRUE
-- inferred: tuple[int, decimal.Decimal | None]
```

```sql
SELECT c.n FROM users LEFT JOIN (SELECT count(*) AS n FROM orders) c ON TRUE
-- inferred: tuple[int]
```

```sql
SELECT jsonb_build_object('n', c.n) AS doc FROM users
LEFT JOIN LATERAL (SELECT count(*) AS n FROM orders WHERE orders.user_id = users.id) c ON TRUE
-- json: doc: {n: integer}
```

A condition other than `TRUE` can fail, and a grouped query can return no row.

```sql
SELECT c.n FROM users LEFT JOIN (SELECT count(*) AS n FROM orders) c ON c.n > 0
-- inferred: tuple[int | None]
```

```sql
SELECT c.n FROM users
LEFT JOIN LATERAL (SELECT count(*) AS n FROM orders WHERE orders.user_id = users.id GROUP BY orders.user_id) c ON TRUE
-- inferred: tuple[int | None]
```

```sql
SELECT c.n FROM users
LEFT JOIN LATERAL (SELECT count(*) AS n FROM orders HAVING count(*) > 1) c ON TRUE
-- inferred: tuple[int | None]
```

## Outer joins through a foreign key

A `NOT NULL` foreign key always references a row, so a `LEFT JOIN` from the
referencing table to the referenced one on exactly the key's columns always matches.
The referenced table's own nullable columns stay nullable.

`schema.sql`:

```sql
CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL, motto TEXT, region TEXT, code TEXT, UNIQUE (region, code));
CREATE TABLE players (
    id INTEGER PRIMARY KEY,
    team_id INTEGER NOT NULL REFERENCES teams (id),
    coach_id INTEGER REFERENCES teams (id),
    region TEXT NOT NULL,
    code TEXT NOT NULL,
    FOREIGN KEY (region, code) REFERENCES teams (region, code)
);
CREATE TABLE fans (id INTEGER PRIMARY KEY, team_id INTEGER NOT NULL);
ALTER TABLE fans ADD CONSTRAINT fans_team FOREIGN KEY (team_id) REFERENCES teams;
```

```sql
SELECT t.name, t.motto FROM players p LEFT JOIN teams t ON t.id = p.team_id
-- inferred: tuple[str, str | None]
```

```sql
SELECT t.name FROM players p LEFT JOIN teams t ON p.region = t.region AND p.code = t.code
-- inferred: tuple[str]
```

```sql
SELECT t.name FROM teams t RIGHT JOIN fans f ON f.team_id = t.id
-- inferred: tuple[str]
```

### When the join can miss

A nullable key column, a condition that is not exactly the key, a referenced relation
that is not the table itself, and a referencing row that is itself null-extended can
all leave the joined row NULL.

```sql
SELECT t.name FROM players p LEFT JOIN teams t ON t.id = p.coach_id
-- inferred: tuple[str | None]
```

```sql
SELECT t.name FROM players p LEFT JOIN teams t ON t.id = p.team_id AND t.motto IS NOT NULL
-- inferred: tuple[str | None]
```

```sql
SELECT t.name FROM players p LEFT JOIN teams t ON p.region = t.region
-- inferred: tuple[str | None]
```

```sql
SELECT t.name FROM players p LEFT JOIN (SELECT * FROM teams WHERE motto IS NOT NULL) t ON t.id = p.team_id
-- inferred: tuple[str | None]
```

```sql
WITH teams AS (SELECT * FROM teams WHERE motto IS NOT NULL)
SELECT t.name FROM players p LEFT JOIN teams t ON t.id = p.team_id
-- inferred: tuple[str | None]
```

```sql
SELECT t.name FROM fans f LEFT JOIN players p ON p.id = f.id LEFT JOIN teams t ON t.id = p.team_id
-- inferred: tuple[str | None]
```

### A key that may not hold

A `DEFERRABLE` key is checked at commit, so a transaction can see a row that
references nothing yet. A key added `NOT VALID` is not checked on the rows already
there until `VALIDATE CONSTRAINT`. A query may not see a row of a table with row-level
security, whatever references it.

`schema.sql`:

```sql
CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE secrets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
ALTER TABLE secrets ENABLE ROW LEVEL SECURITY;
CREATE TABLE players (
    id INTEGER PRIMARY KEY,
    team_id INTEGER NOT NULL REFERENCES teams (id) DEFERRABLE INITIALLY DEFERRED,
    secret_id INTEGER NOT NULL REFERENCES secrets (id),
    former_id INTEGER NOT NULL,
    next_id INTEGER NOT NULL
);
ALTER TABLE players ADD CONSTRAINT former FOREIGN KEY (former_id) REFERENCES teams (id) NOT VALID;
ALTER TABLE players ADD CONSTRAINT next FOREIGN KEY (next_id) REFERENCES teams (id) NOT VALID;
ALTER TABLE players VALIDATE CONSTRAINT next;
```

```sql
SELECT t.name, s.name, f.name, n.name FROM players p
LEFT JOIN teams t ON t.id = p.team_id
LEFT JOIN secrets s ON s.id = p.secret_id
LEFT JOIN teams f ON f.id = p.former_id
LEFT JOIN teams n ON n.id = p.next_id
-- inferred: tuple[str | None, str | None, str | None, str]
```

### SQLite

SQLite checks foreign keys only after `PRAGMA foreign_keys = ON`.

```toml
[tool.sqlty]
dialect = "sqlite"
```

`schema.sql`:

```sql
CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE players (id INTEGER PRIMARY KEY, team_id INTEGER NOT NULL REFERENCES teams (id));
```

```sql
SELECT t.name FROM players p LEFT JOIN teams t ON t.id = p.team_id
-- inferred: tuple[str | None]
```

### MySQL

MySQL checks foreign keys in InnoDB tables, its default engine, and ignores them in
the others.

```toml
[tool.sqlty]
dialect = "mysql"
```

`schema.sql`:

```sql
CREATE TABLE teams (id INT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE players (id INT PRIMARY KEY, team_id INT NOT NULL, FOREIGN KEY (team_id) REFERENCES teams (id));
CREATE TABLE fans (id INT PRIMARY KEY, team_id INT NOT NULL, FOREIGN KEY (team_id) REFERENCES teams (id)) ENGINE = MyISAM;
```

```sql
SELECT t.name FROM players p LEFT JOIN teams t ON t.id = p.team_id
-- inferred: tuple[str]
```

```sql
SELECT t.name FROM fans f LEFT JOIN teams t ON t.id = f.team_id
-- inferred: tuple[str | None]
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

## A CASE branch narrows the JSON it builds

A JSON object built only when the joined row exists has its `NOT NULL` columns non-null;
`ELSE NULL` makes the column nullable, not its keys.

```sql
SELECT CASE WHEN orders.id IS NOT NULL THEN json_build_object('id', orders.id, 'user_id', orders.user_id, 'total', orders.total) ELSE NULL END AS doc
FROM users LEFT JOIN orders ON orders.user_id = users.id
-- json: doc: {id: integer, user_id: integer, total: number | null}
```

A guard on a nullable column of the joined row narrows that column and proves the row
present.

```sql
SELECT CASE WHEN orders.total IS NOT NULL THEN json_build_object('id', orders.id, 'total', orders.total, 'note', orders.note) END AS doc
FROM users LEFT JOIN orders ON orders.user_id = users.id
-- json: doc: {id: integer, total: number, note: string | null}
```

A guard on another relation does not narrow the joined row.

```sql
SELECT CASE WHEN users.age IS NOT NULL THEN json_build_object('id', orders.id, 'user_id', orders.user_id) END AS doc
FROM users LEFT JOIN orders ON orders.user_id = users.id
-- json: doc: {id: integer | null, user_id: integer | null}
```

Later branches see the earlier conditions fail.

```sql
SELECT CASE WHEN orders.id IS NULL THEN NULL ELSE json_build_object('id', orders.id) END AS doc
FROM users LEFT JOIN orders ON orders.user_id = users.id
-- json: doc: {id: integer}
```

```python
from typing import Annotated, LiteralString, TypedDict
from sqlty import SQL

def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T: ...

class Order(TypedDict):
    id: int
    user_id: int

class Row(TypedDict):
    doc: Order | None

row = fetch_one("SELECT CASE WHEN orders.id IS NOT NULL THEN json_build_object('id', orders.id, 'user_id', orders.user_id) ELSE NULL END AS doc FROM users LEFT JOIN orders ON orders.user_id = users.id", Row)
# error:
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
