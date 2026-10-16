# SELECT * Checks

## Star with no schema is an error

```python
from sqlty import SQL
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
```

## Star with no schema reports no stale type

Without a schema the columns of `*` are unknown, so inference gives no type
and the annotation is not compared with a guess.

```python
from sqlty import SQL, UNTYPED
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
# error: [select-star-no-schema] "SELECT * used but no schema available"
r = SQL[tuple[int, int]]("--sql\nSELECT id, * FROM users")
# error: [select-star-no-schema] "SELECT u.* used but no schema available"
s = SQL[tuple[int]]("--sql\nSELECT u.* FROM users u")
# error: [select-star-no-schema] "SELECT * used but no schema available"
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
t = SQL[UNTYPED]("--sql\nSELECT id::INTEGER, * FROM users")
```

## Star with schema produces warning

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255) NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```python
from sqlty import SQL
# error: [select-star] "SELECT * used — expand to explicit column list"
q = SQL[tuple[int, str, str]]("--sql\nSELECT * FROM users")
```

## Star with schema and rule disabled produces no error

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
[tool.sqlty.rules]
select-star = "ignore"
```

```python
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# error:
```

## Explicit columns produce no error

```python
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error:
```

## Every spelling of a result-column star is reported

A qualified `u.*`, a star after other columns, and lowercase or extra
whitespace are all stars. The diagnostic names the stars of each clause.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER NOT NULL, user_id INTEGER NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error: [select-star] "SELECT u.* used — expand to explicit column list"
a = SQL[tuple[int, str]]("--sql\nSELECT u.* FROM users u")
# error: [select-star] "SELECT * used — expand to explicit column list"
b = SQL[tuple[int, int, str]]("--sql\nSELECT id, * FROM users")
# error: [select-star] "SELECT * used — expand to explicit column list"
c = SQL[tuple[int, str]]("--sql\nselect  * from users")
# error: [select-star] "SELECT u.* used — expand to explicit column list"
d = SQL[tuple[int, str, int]]("--sql\nSELECT u.*, o.id FROM users u JOIN orders o ON o.user_id = u.id")
# error: [select-star] "SELECT u.*, o.* used — expand to explicit column list"
e = SQL[tuple[int, str, int, int]]("--sql\nSELECT u.*, o.* FROM users u JOIN orders o ON o.user_id = u.id")
# error: [select-star] "SELECT u.* used — expand to explicit column list"
f = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users UNION SELECT u.* FROM users u")
# error: [select-star] "SELECT d.* used — expand to explicit column list"
g = SQL[tuple[int, str]]("--sql\nSELECT d.* FROM (SELECT id, name FROM users) d")
# error: [select-star] "SELECT public.users.* used — expand to explicit column list"
h = SQL[tuple[int, str]]("--sql\nSELECT public.users.* FROM public.users")
```

## Star in RETURNING is reported

`RETURNING *` shapes the row type of `INSERT`, `UPDATE` and `DELETE` like
`SELECT *` does.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error: [select-star] "RETURNING * used — expand to explicit column list"
a = SQL[tuple[int, str]]("--sql\nINSERT INTO users (id, name) VALUES (1, 'a') RETURNING *")
# error: [select-star] "RETURNING users.* used — expand to explicit column list"
b = SQL[tuple[int, str]]("--sql\nUPDATE users SET name = 'b' WHERE id = 1 RETURNING users.*")
# error: [select-star] "RETURNING * used — expand to explicit column list"
c = SQL[tuple[int, str]]("--sql\nDELETE FROM users WHERE id = 1 RETURNING *")
d = SQL[tuple[int]]("--sql\nINSERT INTO users (id, name) VALUES (1, 'a') RETURNING id")
```

## Star in a CTE is reported

A CTE's columns flow into the query that reads it, so a star there changes
the result as much as one in the outer `SELECT`. This includes a
data-modifying CTE's `RETURNING *`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error: [select-star] "WITH u AS (SELECT *) used — expand to explicit column list"
a = SQL[tuple[int]]("--sql\nWITH u AS (SELECT * FROM users) SELECT id FROM u")
# error: [select-star] "WITH d AS (RETURNING *) used — expand to explicit column list"
b = SQL[tuple[int]]("--sql\nWITH d AS (DELETE FROM users RETURNING *) SELECT id FROM d")
# error: [select-star] "WITH u AS (SELECT u.*) and SELECT * used — expand to explicit column list"
c = SQL[tuple[int, str]]("--sql\nWITH u AS (SELECT u.* FROM users u) SELECT * FROM u")
```

## A star outside the result columns is not reported

A star in `EXISTS` or `count(*)` does not shape the row type.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

a = SQL[tuple[int]]("--sql\nSELECT id FROM users WHERE EXISTS (SELECT * FROM users)")
b = SQL[tuple[int]]("--sql\nSELECT count(*)::INTEGER FROM users")
# error:
```
