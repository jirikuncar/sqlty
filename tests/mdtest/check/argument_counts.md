# Argument counts

`argument-count-mismatch` reports a call of a built-in function whose arguments fit none of its
overloads, wherever it is in the query.

## PostgreSQL

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE FUNCTION lpad(n INTEGER) RETURNS TEXT LANGUAGE sql AS $$ SELECT n::text $$;
```

```python
from sqlty import SQL

# error: [argument-count-mismatch] "`lower` takes 1 argument, got 2: lower(text) -> text; lower(anyrange) -> anyelement; lower(anymultirange) -> anyelement"
q = SQL[tuple[str | None]]("--sql\nSELECT lower(name, 1) FROM users")
# error: [argument-count-mismatch] "`upper` takes 1 argument, got 0: upper(text) -> text; upper(anyrange) -> anyelement; upper(anymultirange) -> anyelement"
q = SQL[tuple[int | None]]("--sql\nSELECT id FROM users WHERE upper() = ''")
# error: [argument-count-mismatch] "`lower` takes no `*`, only `count` does: lower(text) -> text; lower(anyrange) -> anyelement; lower(anymultirange) -> anyelement"
q = SQL[tuple[int | None]]("--sql\nSELECT id FROM users ORDER BY lower(*)")
# error: [argument-count-mismatch] "`lower` takes no `*`, only `count` does: lower(text) -> text; lower(anyrange) -> anyelement; lower(anymultirange) -> anyelement"
q = SQL("--sql\nSELECT lower(*) FROM users")
# error: [argument-count-mismatch] "`ltrim` takes 1 or 2 arguments, got 3: ltrim(string: text[, characters: text]) -> text; ltrim(bytes: bytea, bytesremoved: bytea) -> bytea"
q = SQL[tuple[int | None]]("--sql\nSELECT id FROM users WHERE ltrim(name, 'x', 'y') = ''")
```

Optional and repeated parameters, `count(*)`, named arguments and a function the schema defines
are not reported.

```python
from sqlty import SQL

# error:
q = SQL[tuple[str | None, str | None, str, str | None]](
    "--sql\nSELECT ltrim(name), ltrim(name, 'x'), concat(name, name, name), lpad(id) FROM users"
)
# error:
q = SQL[tuple[int]]("--sql\nSELECT count(*) FROM users")
q = SQL[tuple[str]]("--sql\nSELECT make_interval(days => 1)::text")
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

# error: [argument-count-mismatch] "`left` takes 2 arguments, got 1: left(str, len)"
q = SQL[tuple[str | None]]("-- sql\nSELECT LEFT(name) FROM users")
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

# error: [argument-count-mismatch] "`lower` takes 1 argument, got 2: lower(string: VARCHAR) -> VARCHAR"
q = SQL[tuple[str | None]]("--sql\nSELECT lower(name, name) FROM users")
```

## SQLite

SQLite has no function catalog, so nothing is reported.

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

# error:
q = SQL[tuple[str | None]]("--sql\nSELECT lower(name, 1) FROM users")
```
