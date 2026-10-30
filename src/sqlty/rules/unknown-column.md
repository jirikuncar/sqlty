# unknown-column

Default severity: **error**

## What it does

Reports a column that the table it is read from does not have: in the select
list, `WHERE` and `JOIN ... ON`, and in what `INSERT`, `UPDATE` and `DELETE`
write and read. The table must be known: a column of an unknown table is
covered by `unknown-table` instead.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-column = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unknown-column] "column `email` does not exist in `users`"
q = SQL[tuple[int, str]]("--sql\nSELECT id, email FROM users")
```

### The closest column

When one column of the tables in scope is closest to the unknown name, the message
names it, and `sqlty check --fix` or the editor's quick fix ("Fix unknown-column")
writes it. Close means at most one edit (a typed, missing, changed or swapped character)
per three characters of the longer name, or one name inside the other when the shorter
has four characters or more: `surname` holds `name`. Case is ignored. A qualified name
(`u.surname`) only looks in its own table. A tie suggests nothing: sqlty never guesses
which column you meant.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT, email TEXT);
```

```python
from sqlty import SQL

# error: [unknown-column] "column `surname` does not exist in `users`; did you mean `name`?"
q = SQL[tuple[int, str]]("--sql\nSELECT id, surname FROM users")
```

```python
from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id, surname FROM users")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int | None, str | None]]("--sql\nSELECT id, name FROM users")
```

### Composite fields

A field of a composite type the schema declares is checked the same way.

`schema.sql`:

```sql
CREATE TYPE address AS (street TEXT, zip TEXT);
CREATE TABLE users (id INTEGER, home address);
```

```python
from sqlty import SQL

# error: [unknown-column] "type `address` has no field `city`"
q = SQL[tuple[int, str]]("--sql\nSELECT id, (home).city FROM users")
```

## Why is this bad?

An unknown column has no type, so its annotation is not checked, and the query fails when it runs.

## How to fix

Apply the suggested column with `sqlty check --fix`, fix the name, or add the column to the schema file. Columns of CTEs and derived tables, `USING`/`NATURAL` join columns, outer-query columns in correlated subqueries and `LATERAL`, and whole-row references (`SELECT u FROM users u`) are resolved and not reported. A column that may come from a table function or another source whose columns are not read is reported as `unsupported-syntax`. To accept one case, put `# sqlty: ignore[unknown-column]` on its line.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
unknown-column = "warn"
```

Only checked when a schema with tables is configured. Columns are resolved in the select list, `WHERE`, `JOIN ... ON`, `GROUP BY`, `HAVING`, `ORDER BY`, and what `INSERT`, `UPDATE` and `DELETE` write; a result column's alias or position in `GROUP BY` and `ORDER BY` is not a table column. Applies to the `postgres`, `mysql` and `sqlite` dialects.
