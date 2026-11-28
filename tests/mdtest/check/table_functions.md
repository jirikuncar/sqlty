# Unknown table functions

A function in `FROM` that is neither one of the dialect's built-ins nor one the schema defines is
`unknown-function`, with the table function it most likely misspells, which `--fix` writes.
Without a close name the function may be an extension's the schema does not declare, so
`unsupported-syntax` still says sqlty does not read its rows.

## PostgreSQL

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE FUNCTION active_users() RETURNS TABLE (id INTEGER) LANGUAGE sql AS $$ SELECT id FROM users $$;
```

```python
from sqlty import SQL

# error: [unknown-function] "table function `generate_seriez` is not known to sqlty; did you mean `generate_series`?"
q = SQL("--sql\nSELECT * FROM generate_seriez(1, 3)")
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
# error: [unknown-function] "table function `jsonb_each_txt` is not known to sqlty; did you mean `jsonb_each_text`?"
r = SQL("--sql\nSELECT u.id FROM users u CROSS JOIN LATERAL jsonb_each_txt(u.name::jsonb) AS e")
# error: [unknown-function] "table function `no_such_rows` is not known to sqlty"
# error: [unsupported-syntax] "`*` reads table function no_such_rows, which sqlty does not read the columns of"
s = SQL("--sql\nSELECT * FROM no_such_rows(1)")
```

```python
from sqlty import SQL

q = SQL("--sql\nSELECT * FROM generate_seriez(1, 3)")
# fixed: from sqlty import SQL
# fixed:
# fixed: q = SQL[tuple[int]]("--sql\nSELECT * FROM generate_series(1, 3)")
```

A built-in, a function the schema defines and a qualified name are not reported.

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list"
q = SQL[tuple[int]]("--sql\nSELECT * FROM generate_series(1, 3)")
# error: [select-star] "SELECT * used — expand to explicit column list"
r = SQL[tuple[int | None]]("--sql\nSELECT * FROM active_users()")
# error: [unsupported-syntax] "`*` reads table function rows_of, which sqlty does not read the columns of"
s = SQL("--sql\nSELECT * FROM other.rows_of(1)")
```

## DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unsupported-syntax] "`*` reads table function read_csvv, which sqlty does not read the columns of"
q = SQL("--sql\nSELECT * FROM read_csvv('users.csv')")
```

## MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unknown-function] "table function `generate_series` is not known to sqlty"
# error: [unsupported-syntax] "`*` reads table function generate_series, which sqlty does not read the columns of"
q = SQL("-- sql\nSELECT * FROM generate_series(1, 3)")
```

## SQLite

SQLite has no function catalog, so its table functions are not checked.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unsupported-syntax] "`*` reads table function generate_seriez, which sqlty does not read the columns of"
q = SQL("--sql\nSELECT * FROM generate_seriez(1, 3)")
```
