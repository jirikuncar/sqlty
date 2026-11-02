# Completion

`core.complete_source` lists the table and column names that can be typed at a cursor in
a query string. A string gets completion exactly when `sqlty check` checks it. The
`<CURSOR>` marks where the cursor is; `# completions:` lists `label kind detail`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total NUMERIC);
CREATE VIEW user_names AS SELECT id, name FROM users;
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

## Annotated helper

A string passed to a parameter marked `Annotated[LiteralString, SQL]` is a query, with
no `--sql` marker.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T]) -> list[T]: ...

fetch_all("SELECT <CURSOR> FROM users", tuple[int, str])
# completions: email column users
# completions: id column users
# completions: name column users
```

## Configured function

A function registered in `[[tool.sqlty.functions]]` needs no marked parameter; the
query can be passed by keyword.

```toml
[[tool.sqlty.functions]]
import-path = "pkg.db.fetch_all"
query-param = "query"
return-type-param = "form"
```

`pkg/db.py`:

```python
from typing import Any, LiteralString

def fetch_all[T](query: LiteralString, form: type[T]) -> list[T]: ...
```

```python
from pkg.db import fetch_all

fetch_all(query="SELECT o.<CURSOR> FROM users u JOIN orders o ON o.user_id = u.id", form=tuple[int])
# completions: id column o
# completions: total column o
# completions: user_id column o
```

## Method

A method marked on its class is a query on a receiver whose class is evident.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

class Db:
    def fetch_one[T](self, query: Annotated[LiteralString, SQL], form: type[T]) -> T: ...

db = Db()
db.fetch_one("SELECT name FROM users WHERE <CURSOR> = 1", str)
# completions: email column users
# completions: id column users
# completions: name column users
```

## Adjacent literals

Adjacent string literals are one query; a table position completes relation names.

```python
q = (
    "--sql\n"
    "SELECT count(*) "
    "FROM us<CURSOR>"
)
# completions: user_names view public
# completions: users table public
```

## Marker and wrapper

A `--sql` string, wrapped in `SQL[T](...)` or not. The schema's relations come before
PostgreSQL's system catalogs, which a bare name also finds.

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT count(*) FROM <CURSOR>")
# completions: orders table public
# completions: user_names view public
# completions: users table public
# completions: pg_attrdef table pg_catalog
# completions: pg_attribute table pg_catalog
# completions: pg_available_extension_versions table pg_catalog
# completions: pg_available_extensions table pg_catalog
# completions: pg_backend_memory_contexts table pg_catalog
# completions: pg_class table pg_catalog
# completions: pg_config table pg_catalog
# completions: ...
```

## Prefix

Only names that start with the word before the cursor complete.

```python
q = "--sql\nSELECT * FROM users u WHERE u.e<CURSOR>"
# completions: email column u
```

## Same column in several tables

A column offered by several tables is listed once per alias; the editor shows the alias
next to each entry. A table named twice without aliases lists its columns once.

```python
q = "--sql\nSELECT <CURSOR> FROM users u JOIN users v ON u.id = v.id"
# completions: email column u
# completions: email column v
# completions: id column u
# completions: id column v
# completions: name column u
# completions: name column v
```

```python
q = "--sql\nSELECT <CURSOR> FROM users, users"
# completions: email column users
# completions: id column users
# completions: name column users
```

## CTE and subquery scope

```python
q = """--sql
WITH recent AS (SELECT id, total AS amount FROM orders)
SELECT r.<CURSOR> FROM recent r
"""
# completions: amount column r
# completions: id column r
```

## Does not complete

### Unregistered callee

```python
def run(query: str) -> None: ...

run("SELECT <CURSOR> FROM users")
# completions: None
```

### f-string

```python
table = "users"
q = f"--sql\nSELECT <CURSOR> FROM {table}"
# completions: None
```

### Variable

```python
query = "SELECT id FROM users"
run(query<CURSOR>)
# completions: None
```

### Alias position

```python
q = "--sql\nSELECT * FROM users <CURSOR>"
# completions: None
```

## MySQL and SQLite

Names compare without case, and a table name qualifies its columns.

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "-- sql\nSELECT Users.<CURSOR> FROM Users"
# completions: email column users
# completions: id column users
# completions: name column users
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
q = "--sql\nSELECT * FROM US<CURSOR>"
# completions: user_names view -
# completions: users table -
```
