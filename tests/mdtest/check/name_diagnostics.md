# Lint: name diagnostics

The rules `unknown-table`, `unknown-column`, `ambiguous-column`,
`unknown-function`, `unknown-type` and `unsupported-syntax` explain why a
result column is untyped. The first three are errors by default, the others
warnings (see the user guide in `docs/guide/name-diagnostics.md`).

## With a schema

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, team_id INTEGER, "Mixed" TEXT, shape GEOMETRY);
CREATE TABLE teams (id INTEGER PRIMARY KEY, title TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER, total NUMERIC(10, 2));
```

### On by default

```python
from sqlty import SQL

# error: [unknown-table] "table `missing` is not defined in the schema"
q = SQL[tuple[int]]("--sql\nSELECT nope FROM missing")
```

### Enabled rules

```toml
[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
ambiguous-column = "error"
unknown-function = "error"
unknown-type = "error"
unsupported-syntax = "error"
select-star = "ignore"
```

#### Unknown names

```python
from sqlty import SQL

# error: [unknown-column] "column `nope` does not exist in `users`"
a = SQL[tuple[int]]("--sql\nSELECT nope FROM users")
# error: [unknown-column] "column `nope` does not exist in `u`"
b = SQL[tuple[int]]("--sql\nSELECT u.nope FROM users u")
# error: [unknown-table] "`x` is not a table or alias in the FROM clause"
c = SQL[tuple[int]]("--sql\nSELECT x.id FROM users u")
# error: [unknown-table] "table `missing` is not defined in the schema"
d = SQL[tuple[int]]("--sql\nSELECT id FROM missing")
# error: [unknown-table] "table `missing` is not defined in the schema"
e = SQL[tuple[int]]("--sql\nSELECT * FROM missing")
# error: [unknown-table] "table `missing` is not defined in the schema"
f = SQL[tuple[int]]("--sql\nINSERT INTO missing (a) VALUES (1) RETURNING a")
```

#### GROUP BY, HAVING and ORDER BY

Their keys are checked like `WHERE`, except a result column named by its alias or
position, which `GROUP BY` and `ORDER BY` accept. PostgreSQL reads no aliases in `HAVING`.
A bare name in `ORDER BY` reads a result column first, aliased or not, so `id` in `g`
is `t.id` even though both tables have the column. Without such a result column, in `h`,
it reads the tables and is ambiguous.

```toml
[tool.sqlty.rules]
stale-rewrite = "ignore"
```

```python
# error: [unknown-column] "column `nmae` does not exist in `users`"
a = "--sql\nSELECT id FROM users ORDER BY nmae"
# error: [unknown-column] "column `nmae` does not exist in `users`"
b = "--sql\nSELECT name FROM users GROUP BY name, nmae"
# error: [unknown-column] "column `nope` does not exist in `users`"
c = "--sql\nSELECT name, count(*) AS n FROM users GROUP BY name HAVING count(nope) > 1"
# error: [unknown-column] "column `n` does not exist in `users`"
d = "--sql\nSELECT name, count(*) AS n FROM users GROUP BY name HAVING n > 1"
e = "--sql\nSELECT name AS label, count(*) AS n FROM users GROUP BY label ORDER BY n DESC, 1"
f = "--sql\nSELECT u.name FROM users u ORDER BY u.id"
g = "--sql\nSELECT t.id, t.title FROM teams t JOIN users u ON u.team_id = t.id ORDER BY title, id"
# error: [ambiguous-column] "column reference `id` is ambiguous: `t`, `u` all have it"
h = "--sql\nSELECT t.title FROM teams t JOIN users u ON u.team_id = t.id ORDER BY id"
```

#### Result column aliases in SQLite and MySQL

SQLite reads a result column's alias anywhere, `WHERE` included; MySQL in `GROUP BY`,
`HAVING` and `ORDER BY` expressions, not in `WHERE`. PostgreSQL only reads a bare
alias in `GROUP BY` and `ORDER BY`.

##### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"

[tool.sqlty.rules]
stale-rewrite = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
# error:
a = "--sql\nSELECT name AS n FROM users WHERE n = 'a' GROUP BY n || 'x' ORDER BY lower(n)"
```

##### MySQL

```toml
[tool.sqlty]
dialect = "mysql"

[tool.sqlty.rules]
stale-rewrite = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);
```

```python
# error: [unknown-column] "column `n` does not exist in `users`"
a = "-- sql\nSELECT name AS n FROM users WHERE n = 'a' ORDER BY lower(n)"
```

A user variable (`@name`) or a system variable (`@@name`) is not a column:

```python
# error:
b = "-- sql\nSELECT name, @@version FROM users WHERE id = @uid"
```

#### INSERT, UPDATE and DELETE

A statement that returns no rows is checked too: the table it writes, the
columns it sets, and the names its values, conditions and source query read.

```python
# error: [insert-missing-column] "INSERT into `users` leaves out `name`, which is NOT NULL and has no default"
# error: [unknown-column] "column `nmae` does not exist in `users`"
a = "--sql\nINSERT INTO users (id, nmae) VALUES (1, 'x')"
# error: [unknown-table] "table `missing` is not defined in the schema"
b = "--sql\nINSERT INTO missing (a) VALUES (1)"
# error: [unknown-column] "column `nope` does not exist in `users`"
c = "--sql\nINSERT INTO orders (id, user_id) SELECT id, nope FROM users"
# error: [unknown-column] "column `nmae` does not exist in `users`"
# error: [unknown-column] "column `nope` does not exist in `teams`, `users`"
d = "--sql\nUPDATE users SET nmae = 'x', team_id = (SELECT nope FROM teams) WHERE id = 1"
# error: [unknown-column] "column `idd` does not exist in `u`"
e = "--sql\nUPDATE users AS u SET name = 'x' WHERE u.idd = 1"
# error: [unknown-column] "column `titel` does not exist in `teams`"
f = "--sql\nDELETE FROM teams WHERE titel = 'x'"
g = "--sql\nDELETE FROM orders WHERE user_id IN (SELECT id FROM users WHERE team_id = 1)"
```

A query's `WHERE` and `JOIN ... ON` are checked as well, not only the columns
that shape its result. `UPDATE ... FROM` and `DELETE ... USING` read the joined
tables too.

```python
from sqlty import SQL

# error: [unknown-column] "column `idd` does not exist in `t`"
# error: [unknown-column] "column `nmae` does not exist in `u`"
h = SQL[tuple[int]]("--sql\nSELECT u.id FROM users u JOIN teams t ON t.idd = u.team_id WHERE u.nmae = 'x'")
# error: [unknown-column] "column `nope` does not exist in `t`"
i = "--sql\nUPDATE users u SET name = t.title FROM teams t WHERE t.id = u.team_id AND t.nope"
j = "--sql\nDELETE FROM orders o USING users u WHERE u.id = o.user_id AND u.name = 'x'"
```

An `INSERT` target may have an alias, and PostgreSQL's `ON CONFLICT DO UPDATE` reads
the target and `EXCLUDED`, the row that was not inserted.

```python
from sqlty import SQL

k = SQL[tuple[str]]("--sql\nINSERT INTO users AS u (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name WHERE u.name <> EXCLUDED.name RETURNING u.name")
# error: [unknown-column] "column `nope` does not exist in `excluded`"
# error: [unknown-column] "column `nmae` does not exist in `users`"
# error: [unknown-column] "column `titel` does not exist in `users`"
l = "--sql\nINSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET titel = EXCLUDED.nope WHERE users.nmae IS NULL"
# error: [ambiguous-column] "column reference `name` is ambiguous: `users`, `excluded` all have it"
m = "--sql\nINSERT INTO users (id, name) VALUES (1, 'a') ON CONFLICT (id) DO UPDATE SET name = name || EXCLUDED.name"
```

A subquery over the same table as the outer query names the table once.

```python
from sqlty import SQL

# error: [unknown-column] "column `nope` does not exist in `users`"
g = SQL[tuple[int]]("--sql\nSELECT (SELECT nope FROM users) FROM users")
```

#### Ambiguous columns

```python
from sqlty import SQL

# error: [ambiguous-column] "column reference `id` is ambiguous: `u`, `t` all have it"
a = SQL[tuple[int]]("--sql\nSELECT id FROM users u JOIN teams t ON t.id = u.team_id")
# error: [ambiguous-column] "column reference `id` is ambiguous: `u1`, `u2` all have it"
b = SQL[tuple[int]]("--sql\nSELECT id FROM users u1 JOIN users u2 ON u1.id = u2.id")
```

#### Functions, types and syntax

```python
from sqlty import SQL

# error: [unknown-function] "function `frobnicate` is not known to sqlty"
a = SQL[tuple[int]]("--sql\nSELECT frobnicate(id) FROM users")
# error: [unknown-type] "type `mood` has no Python type"
b = SQL[tuple[str]]("--sql\nSELECT name::mood FROM users")
# error: [unknown-type] "column `users.shape` has type `GEOMETRY`"
c = SQL[tuple[str]]("--sql\nSELECT shape FROM users")
# error: [unsupported-syntax] "a TABLE query is not typed"
d = SQL[tuple[int]]("--sql\nSELECT 1 UNION TABLE users")
# error: [unsupported-syntax] "`*` over a USING or NATURAL join merges columns"
e = SQL[tuple[int, str, int, str]]("--sql\nSELECT * FROM users JOIN teams USING (id)")
# error: [unsupported-syntax] "expression `1 OPERATOR(pg_catalog.+) 1` is not typed"
f = SQL[tuple[object]]("--sql\nSELECT 1 OPERATOR(pg_catalog.+) 1")
```

#### Annotated helpers

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_value[T](query: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...

# error: [unknown-column] "column `nope` does not exist in `users`"
a = fetch_value("SELECT nope FROM users", int)
# error:
b = fetch_value("SELECT id FROM users", int)
```

#### Suppressed inline

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT nope FROM users")  # sqlty: ignore[unknown-column]
b = SQL[tuple[int]]("--sql\nSELECT nope FROM missing")  # sqlty: ignore
```

### Only one rule enabled

```toml
[tool.sqlty.rules]
unknown-table = "ignore"
unknown-column = "warn"
```

```python
from sqlty import SQL

# error: [unknown-column] "column `nope` does not exist in `users`"
a = SQL[tuple[int]]("--sql\nSELECT nope FROM users")
b = SQL[tuple[int]]("--sql\nSELECT id FROM missing")
```

### Names that resolve are not reported

```toml
[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
ambiguous-column = "error"
unknown-function = "error"
unknown-type = "error"
unsupported-syntax = "error"
```

#### Row constructors

`ROW(...)` and `(a, b)` build a record, not a call to a function `row`.

```python
from typing import Any
from sqlty import SQL

# error:
a = SQL[tuple[tuple[Any, ...]]]("--sql\nSELECT ROW(id, name) AS r FROM users")
# error:
b = SQL[tuple[tuple[Any, ...]]]("--sql\nSELECT (id, name) AS r FROM users")
```

#### CTEs

```python
from sqlty import SQL

# error:
a = SQL[tuple[int, str]]("--sql\nWITH c AS (SELECT id, name FROM users) SELECT id, name FROM c")
# error:
b = SQL[tuple[int, str]]("--sql\nWITH c(a, b) AS (SELECT id, name FROM users) SELECT a, b FROM c")
# error:
c = SQL[tuple[int]]("--sql\nWITH RECURSIVE r(n) AS (SELECT 1::INTEGER UNION ALL SELECT n + 1 FROM r) SELECT n FROM r")
# error:
d = SQL[tuple[int]]("--sql\nWITH c AS (SELECT id FROM users) SELECT c.id FROM c")
# error:
e = SQL[tuple[int]]("--sql\nWITH RECURSIVE r(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM r WHERE n < 5) SELECT n FROM r")
```

#### A recursive CTE over VALUES outside PostgreSQL

The anchor's `VALUES` list is untyped there, but the CTE is still a table.

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
# error: [unsupported-syntax] "a VALUES list is not typed"
q = "--sql\nWITH RECURSIVE r(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM r WHERE n < 5) SELECT n FROM r"
```

#### Aliases

```python
from sqlty import SQL

# error:
a = SQL[tuple[int, str | None]]("--sql\nSELECT u.id, t.title FROM users u JOIN teams t ON t.id = u.team_id")
# error:
b = SQL[tuple[int]]("--sql\nSELECT users.id FROM users, orders")
# error:
c = SQL[tuple[int | None]]("--sql\nSELECT o.id FROM users u JOIN orders o ON o.user_id = u.id")
```

#### Subquery and derived columns

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT d.x FROM (SELECT id AS x FROM users) d")
# error:
b = SQL[tuple[int]]("--sql\nSELECT x FROM (SELECT id AS x FROM users) d")
# error:
c = SQL[tuple[int]]("--sql\nSELECT x FROM (SELECT id FROM users) AS d(x)")
# error:
d = SQL[tuple[int]]("--sql\nSELECT id FROM (SELECT 1::INTEGER AS id) AS s")
```

#### Table functions

The built-in set-returning functions (`generate_series`, `unnest`, the JSON
`*_each` and `*_array_elements` families) have known columns. The columns of
another table function are not read, so a column taken from one is reported
as unsupported, never as an unknown column or table.

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT g FROM generate_series(1, 3) AS g")
b = SQL[tuple[int]]("--sql\nSELECT g.n FROM generate_series(1, 3) AS g(n)")
# error:
c = SQL[tuple[int | None]]("--sql\nSELECT n FROM unnest(ARRAY[1, 2]) AS n")
# error: [unsupported-syntax] "column `word` may come from table function pg_get_keywords, which sqlty does not read the columns of"
e = SQL[tuple[str]]("--sql\nSELECT k.word FROM pg_get_keywords() AS k")
# error: [unsupported-syntax] "column `tgrelid` may come from system catalog pg_trigger, which sqlty does not read the columns of"
d = SQL[tuple[int]]("--sql\nSELECT tgrelid FROM pg_trigger")
```

#### System catalogs

The catalog models PostgreSQL's common system relations, so their columns are
typed and checked like a table's. Other `pg_catalog` and `information_schema`
relations are not read.

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]("--sql\nSELECT relname FROM pg_class")
# error:
b = SQL[tuple[str | None]]("--sql\nSELECT table_name FROM information_schema.tables")
# error:
c = SQL[tuple[int, str]]("--sql\nSELECT c.oid, c.relname FROM pg_catalog.pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace")
# error: [unknown-column] "column `nope` does not exist in `pg_class`"
d = SQL[tuple[int]]("--sql\nSELECT nope FROM pg_class")
```

#### LATERAL

```python
import decimal
from sqlty import SQL

# error:
a = SQL[tuple[int, int]]("--sql\nSELECT u.id, l.n FROM users u, LATERAL (SELECT count(*) AS n FROM orders o WHERE o.user_id = u.id) l")
# error:
b = SQL[tuple[decimal.Decimal | None]]("--sql\nSELECT l.total FROM users u CROSS JOIN LATERAL (SELECT o.total FROM orders o WHERE o.user_id = u.id) l")
```

#### USING and NATURAL joins

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT id FROM users JOIN teams USING (id)")
# error:
b = SQL[tuple[int]]("--sql\nSELECT id FROM users NATURAL JOIN teams")
# error:
c = SQL[tuple[int, str, str | None]]("--sql\nSELECT u.id, u.name, t.title FROM users u JOIN teams t USING (id)")
```

#### Correlated subqueries

```python
import decimal
from sqlty import SQL

# error:
a = SQL[tuple[decimal.Decimal | None]]("--sql\nSELECT (SELECT max(o.total) FROM orders o WHERE o.user_id = u.id) FROM users u")
# error:
b = SQL[tuple[str]]("--sql\nSELECT (SELECT u.name) FROM users u")
```

#### Quoted identifiers and case folding

```python
from sqlty import SQL

# error:
a = SQL[tuple[str | None]]('--sql\nSELECT "Mixed" FROM users')
# error:
b = SQL[tuple[int, str]]("--sql\nSELECT U.ID, NAME FROM Users U")
# error:
c = SQL[tuple[int]]('--sql\nSELECT "id" FROM "users"')
# error:
d = SQL[tuple[int]]("--sql\nSELECT Users.Id FROM USERS")
```

#### Whole-row references, NULL and placeholders

```python
from sqlty import SQL

# error:
a = SQL[tuple[object]]("--sql\nSELECT u FROM users u")
# error:
b = SQL[tuple[int, int]]("--sql\nSELECT NULL, 1::INTEGER")
# error:
c = SQL[tuple[int]]("--sql\nSELECT id FROM users WHERE id = $1")
```

#### DML with RETURNING

```python
from sqlty import SQL

# error:
a = SQL[tuple[int, str]]("--sql\nINSERT INTO users (id, name) VALUES (1, 'a') RETURNING id, name")
# error:
b = SQL[tuple[int]]("--sql\nUPDATE users u SET name = 'a' FROM teams t WHERE t.id = u.team_id RETURNING u.id")
# error:
c = SQL[tuple[str | None]]("--sql\nUPDATE users u SET name = 'a' FROM teams t WHERE t.id = u.team_id RETURNING t.title")
```

#### Known functions and types

```python
import datetime
from sqlty import SQL

# error:
a = SQL[tuple[datetime.datetime, datetime.date, int, str]]("--sql\nSELECT now(), current_date, count(*)::INTEGER, coalesce(name, 'x') FROM users GROUP BY name")
```

#### Named arguments

In `make_interval(days => 3)`, `days` names a parameter of the function, not a column.
The value after `=>` is still checked.

```python
a = "--sql\nDELETE FROM users WHERE now() - make_interval(days => 3) > now()"
b = "--sql\nUPDATE users SET name = 'a' WHERE now() < now() + make_interval(secs => id)"
# error: [unknown-column] "column `nope` does not exist in `users`"
c = "--sql\nDELETE FROM users WHERE now() - make_interval(days => nope) > now()"
```

## Without a schema nothing is guessed

```toml
[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
ambiguous-column = "error"
unknown-function = "error"
unknown-type = "error"
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT nope FROM missing")
# error:
b = SQL[tuple[int]]("--sql\nSELECT id FROM a JOIN b ON a.x = b.x")
# error:
c = SQL[tuple[int]]("--sql\nSELECT my_udf(1)")
# error:
d = SQL[tuple[str]]("--sql\nSELECT 'x'::mood")
```

## Other dialects

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-column = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

A double-quoted name that is not a column is a string literal in SQLite.

```python
from sqlty import SQL

# error:
a = SQL[tuple[str]]('--sql\nSELECT "nope" FROM users')
# error: [unknown-column] "column `nope` does not exist in `users`"
b = SQL[tuple[str]]("--sql\nSELECT nope FROM users")
```

## Views

A view column the view's own query leaves untyped is explained where it is read.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
unsupported-syntax = "error"
select-star = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE VIEW typed AS SELECT id, name FROM users;
CREATE VIEW opaque AS SELECT id, 1 OPERATOR(pg_catalog.+) 1 AS day FROM users;
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int | None, str | None]]("--sql\nSELECT id, name FROM typed")
# error: [unsupported-syntax] "view column `opaque.day` is not typed"
b = SQL[tuple[object]]("--sql\nSELECT day FROM opaque")
# error: [unknown-column] "column `nope` does not exist in `typed`"
c = SQL[tuple[int]]("--sql\nSELECT nope FROM typed")
```

## System columns and ONLY

Columns every table has, and PostgreSQL's `FROM ONLY`, are not unknown.

### postgres

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
unsupported-syntax = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT tableoid FROM users")
b = SQL[tuple[str, str, str, str, str]]("--sql\nSELECT ctid, xmin, xmax, cmin, cmax FROM users")
# error: [unknown-column] "column `oid` does not exist in `users`"
oid = SQL[tuple[int]]("--sql\nSELECT oid FROM users")
# error: [stale-type] "SQL[tuple[int, int]] should be SQL[tuple[int | None, str]]"
c = SQL[tuple[int, int]]("--sql\nSELECT u.id, u.ctid FROM users u")
# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int | None]]"
d = SQL[tuple[int]]("--sql\nSELECT id FROM ONLY users")
# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int | None]]"
e = SQL[tuple[int]]("--sql\nSELECT users.id FROM ONLY users")
# error: [unknown-column] "column `rowid` does not exist in `users`"
f = SQL[tuple[int]]("--sql\nSELECT rowid FROM users")
```

### sqlite

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error:
a = SQL[tuple[int, int, int]]("--sql\nSELECT rowid, oid, _rowid_ FROM users")
# error: [unknown-column] "column `ctid` does not exist in `users`"
b = SQL[tuple[int]]("--sql\nSELECT ctid FROM users")
```

## Stars

A column a star lists without a type is explained like a named one.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-type = "error"
unsupported-syntax = "error"
select-star = "ignore"
```

`schema.sql`:

```sql
CREATE TABLE a (id INTEGER, g GEOMETRY);
CREATE VIEW opaque AS SELECT id, 1 OPERATOR(pg_catalog.+) 1 AS day FROM a;
```

```python
from sqlty import SQL

# error: [unknown-type] "column `a.g` has type `GEOMETRY`, which has no Python type"
x = SQL[tuple[int, str]]("--sql\nSELECT * FROM a")
# error: [unknown-type] "column `a.g` has type `GEOMETRY`, which has no Python type"
y = SQL[tuple[int, str]]("--sql\nSELECT a.* FROM a, generate_series(1, 3) AS g(x)")
# error: [unsupported-syntax] "view column `opaque.day` is not typed by the view's query"
z = SQL[tuple[int, object]]("--sql\nSELECT * FROM opaque")
# error: [unsupported-syntax] "`*` reads table function pg_get_keywords, which sqlty does not read the columns of"
w = SQL[tuple[int, str, int]]("--sql\nSELECT * FROM a, pg_get_keywords() AS k")
```

## Functions and types the schema defines

`unknown-function` and `unknown-type` only fire for names the schema does not
define: functions from `CREATE FUNCTION`, and enums, domains and composite types
from `CREATE TYPE` / `CREATE DOMAIN`, are known.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-function = "error"
unknown-type = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TYPE mood AS ENUM ('happy', 'sad');
CREATE DOMAIN percent AS INTEGER CHECK (VALUE BETWEEN 0 AND 100);
CREATE TYPE address AS (street TEXT, zip TEXT);
CREATE FUNCTION add_one(x INTEGER) RETURNS INTEGER LANGUAGE sql AS 'SELECT x + 1';
CREATE FUNCTION user_ids() RETURNS SETOF INTEGER LANGUAGE sql AS 'SELECT id FROM users';
CREATE FUNCTION opaque() LANGUAGE sql AS 'SELECT 1';
```

```python
import typing
from sqlty import SQL

# error:
a = SQL[tuple[int]]("--sql\nSELECT add_one(id) FROM users")
# error:
b = SQL[tuple[typing.Literal['happy', 'sad']]]("--sql\nSELECT 'happy'::mood")
# error:
c = SQL[tuple[int]]("--sql\nSELECT id::percent FROM users")
# error:
d = SQL[tuple[int]]("--sql\nSELECT user_ids()")
# error:
e = SQL[tuple[object]]("--sql\nSELECT ROW('a', 'b')::address")
# error:
f = SQL[tuple[object]]("--sql\nSELECT opaque()")
# error: [unknown-function] "function `frobnicate` is not known to sqlty"
g = SQL[tuple[int]]("--sql\nSELECT frobnicate(id) FROM users")
# error: [unknown-type] "type `shape` has no Python type"
h = SQL[tuple[int]]("--sql\nSELECT id::shape FROM users")
```

## Composite fields

A composite column has no Python type, so selecting it reports
`unknown-type`. A field read from it, `(p).x`, has the field's type and is
not reported.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-type = "error"
```

`schema.sql`:

```sql
CREATE TYPE point2 AS (x INTEGER, y INTEGER);
CREATE TABLE shapes (id INTEGER PRIMARY KEY, p point2);
```

```python
from sqlty import SQL

a = SQL[tuple[int | None]]("--sql\nSELECT (p).x AS x FROM shapes")
b = SQL[tuple[int | None]]("--sql\nSELECT (shapes.p).y AS y FROM shapes")
# error: [unknown-type] "column `shapes.p` has type `POINT2`, which has no Python type"
c = SQL[tuple[object]]("--sql\nSELECT p FROM shapes")
```
