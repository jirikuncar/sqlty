# select-star

Default severity: **warn**

The examples use this schema:

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE teams (id INTEGER NOT NULL, owner_id INTEGER NOT NULL);
```

## What it does

Flags a `*` or qualified `t.*` that shapes the result of a query whose
columns come from a schema: in the `SELECT` list, in `RETURNING`, or in a
CTE the query reads. Stars that don't shape the result, such as
`EXISTS (SELECT * ...)` or `count(*)`, are not reported.

## Why is this bad?

The row type is fixed when the query is rewritten. Adding a column to the
table silently changes what the query returns at runtime.

## Example

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list (joins need a manual edit)"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# error: [select-star] "SELECT u.* used — expand to explicit column list"
r = SQL[tuple[int, str, int]]("--sql\nSELECT u.*, t.id FROM users u JOIN teams t ON t.owner_id = u.id")
# error: [select-star] "RETURNING * used — expand to explicit column list"
s = SQL[tuple[int, str]]("--sql\nDELETE FROM users WHERE id = 1 RETURNING *")
# error: [select-star] "WITH u AS (SELECT *) used — expand to explicit column list"
t = SQL[tuple[int]]("--sql\nWITH u AS (SELECT * FROM users) SELECT id FROM u")
```

## Fix

`sqlty check --fix .` lists the columns from the schema, each cast to its
type with `::`. MySQL and SQLite have no `::`, so there the columns are listed
bare (`SELECT id, name FROM users`); the schema still types them.

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int, str]]("--sql\nSELECT id::INT, name::TEXT FROM users")
```

A qualified star keeps its qualifier, so it is expanded in a join too:

```python
from sqlty import SQL

r = SQL[tuple[int, str, int]]("--sql\nSELECT u.*, t.id FROM users u JOIN teams t ON t.owner_id = u.id")
# fixed: from sqlty import SQL
# fixed:
# fixed: r = SQL[tuple[int, str, int]]("--sql\nSELECT u.id::INT, u.name::TEXT, t.id FROM users u JOIN teams t ON t.owner_id = u.id")
```

Queries with escapes or quotes inside the string, queries split over several
concatenated strings, and a bare `*` in a query that reads from more than one
table (`JOIN` or `FROM a, b`; its columns would have no qualifier), are left
for you to edit, as are stars in `RETURNING` and in CTEs. `select-star` is
still reported for them.
