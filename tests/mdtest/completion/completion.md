# Completion

`core.complete_source` lists the table, column and function names that can be typed at a cursor in
a query string. A string gets completion exactly when `sqlty check` checks it. The
`<CURSOR>` marks where the cursor is; `# completions:` lists `label kind detail`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users, total NUMERIC);
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

## Join on a foreign key

After `JOIN`, the tables a foreign key links to one in scope come first, either way, each
with the `ON` clause that joins them. A table already in scope under its own name is left
out, since its `ON` clause would be ambiguous.

```python
q = "--sql\nSELECT * FROM users u JOIN <CURSOR>"
# completions: orders ON orders.user_id = u.id join public
# completions: orders table public
# completions: user_names view public
# completions: users table public
# completions: pg_attrdef table pg_catalog
# completions: pg_attribute table pg_catalog
# completions: pg_available_extension_versions table pg_catalog
# completions: pg_available_extensions table pg_catalog
# completions: pg_backend_memory_contexts table pg_catalog
# completions: pg_class table pg_catalog
# completions: ...
```

```python
q = "--sql\nSELECT * FROM orders JOIN us<CURSOR>"
# completions: users ON users.id = orders.user_id join public
# completions: user_names view public
# completions: users table public
```

## Prefix

Only names that start with the word before the cursor complete.

```python
q = "--sql\nSELECT * FROM users u WHERE u.e<CURSOR>"
# completions: email column u
```

## Qualified names

With nothing typed and several relations in scope, each column is offered as
`alias.column`, so the inserted name says whose it is and never turns ambiguous. Once a
prefix is typed, or with one relation in scope, columns are offered bare.

```python
q = "--sql\nSELECT id FROM users u JOIN orders o ON <CURSOR>"
# completions: o.id column o
# completions: o.total column o
# completions: o.user_id column o
# completions: u.email column u
# completions: u.id column u
# completions: u.name column u
```

```python
q = "--sql\nSELECT id FROM users u JOIN orders o ON u.id = o.user_id WHERE to<CURSOR>"
# completions: total column o
# completions: to_ascii function to_ascii(string: text) -> text (+2 more)
# completions: to_bin function to_bin(integer) -> text (+1 more)
# completions: to_char function to_char(timestamp, text) -> text (+3 more)
# completions: to_date function to_date(text, text) -> date
# completions: to_hex function to_hex(integer) -> text (+1 more)
# completions: to_json function to_json(anyelement) -> json
# completions: to_jsonb function to_jsonb(anyelement) -> jsonb
# completions: to_number function to_number(text, text) -> numeric
# completions: to_oct function to_oct(integer) -> text (+1 more)
# completions: ...
```

## Same column in several tables

A column offered by several tables is listed once per alias. A table named twice without
aliases lists its columns once.

```python
q = "--sql\nSELECT <CURSOR> FROM users u JOIN users v ON u.id = v.id"
# completions: u.email column u
# completions: u.id column u
# completions: u.name column u
# completions: v.email column v
# completions: v.id column v
# completions: v.name column v
```

```python
q = "--sql\nSELECT <CURSOR> FROM users, users"
# completions: email column users
# completions: id column users
# completions: name column users
```

## Columns before FROM

With no relation in scope yet, a typed column name completes from every table and view
that has it, each naming its relation, the schema's own before the system catalogs.

```python
q = "--sql\nSELECT na<CURSOR>"
# completions: name column user_names
# completions: name column users
# completions: name column pg_catalog.pg_available_extension_versions
# completions: name column pg_catalog.pg_available_extensions
# completions: name column pg_catalog.pg_backend_memory_contexts
# completions: name column pg_catalog.pg_config
# completions: name column pg_catalog.pg_cursors
# completions: name column pg_catalog.pg_file_settings
# completions: name column pg_catalog.pg_prepared_statements
# completions: name column pg_catalog.pg_settings
# completions: ...
```

```python
q = "--sql\nSELECT id, user_<CURSOR>"
# completions: user_id column orders
# completions: user_defined_type_catalog column information_schema.tables
# completions: user_defined_type_name column information_schema.tables
# completions: user_defined_type_schema column information_schema.tables
# completions: user_name column pg_catalog.pg_hba_file_rules
```

### Names without case

Outside PostgreSQL, names compare without case, those of view columns too.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "views.sql"
```

`views.sql`:

```sql
CREATE TABLE orders (id INT, user_id INT, total DECIMAL(10, 2));
CREATE VIEW Totals AS SELECT user_id AS OwnerId, SUM(total) AS Spent FROM orders GROUP BY user_id;
```

```python
q = "-- sql\nSELECT own<CURSOR>"
# completions: OwnerId column totals
```

### With a FROM

Once the query has a `FROM`, only the columns in scope complete.

```python
q = "--sql\nSELECT user_<CURSOR> FROM users"
# completions: None
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

## Functions

A typed word in an expression also completes to functions, after the columns: those the
schema defines (with their schema), then the dialect's built-ins (with their first
overload). Editors insert `name(`.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);
CREATE FUNCTION normalize_email(email TEXT) RETURNS TEXT LANGUAGE sql AS $$ SELECT lower(email) $$;
CREATE FUNCTION user_rows() RETURNS SETOF users LANGUAGE sql AS $$ SELECT * FROM users $$;
```

```python
q = "--sql\nSELECT coal<CURSOR> FROM users"
# completions: coalesce function coalesce(value: anyelement, ...) -> anyelement
```

```python
q = "--sql\nSELECT id FROM users WHERE NOR<CURSOR>"
# completions: normalize_email function public
```

```python
q = "--sql\nSELECT id FROM users ORDER BY na<CURSOR>"
# completions: name column users
```

In `FROM`, after `JOIN` and after `LATERAL`, a typed word also completes to the functions that
return rows, the schema's then the built-ins, after the relations.

```python
q = "--sql\nSELECT * FROM users, gen<CURSOR>"
# completions: generate_series function generate_series(start: integer, stop: integer[, step: integer]) -> setof integer (+4 more)
# completions: generate_subscripts function generate_subscripts(array: anyarray, dim: integer) -> setof integer (+1 more)
```

```python
q = "--sql\nSELECT * FROM users u CROSS JOIN LATERAL jsonb_each<CURSOR>"
# completions: jsonb_each function jsonb_each(jsonb) -> setof record(key text, value jsonb)
# completions: jsonb_each_text function jsonb_each_text(jsonb) -> setof record(key text, value text)
```

```python
q = "--sql\nSELECT * FROM jsonb_to_rec<CURSOR>"
# completions: jsonb_to_record function jsonb_to_record(jsonb) -> record
# completions: jsonb_to_recordset function jsonb_to_recordset(jsonb) -> setof record
```

```python
q = "--sql\nSELECT * FROM user_r<CURSOR>"
# completions: user_rows function public
```

A function cannot go after `UPDATE` or `DELETE FROM`.

```python
q = "--sql\nUPDATE gen<CURSOR>"
# completions: None
```

```python
q = "--sql\nDELETE FROM user_r<CURSOR>"
# completions: None
```

Nothing typed, or a qualified name, offers no function.

```python
q = "--sql\nSELECT u.low<CURSOR> FROM users u"
# completions: None
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

## Tables by the selected columns

After `FROM`, the relations that have every column the select list names come first,
then those with some of them, then the rest by name.

```python
q = "--sql\nSELECT id, name FROM <CURSOR>"
# completions: user_names view public
# completions: users table public
# completions: orders table public
# completions: pg_available_extension_versions table pg_catalog
# completions: pg_available_extensions table pg_catalog
# completions: pg_backend_memory_contexts table pg_catalog
# completions: pg_config table pg_catalog
# completions: pg_cursors table pg_catalog
# completions: pg_file_settings table pg_catalog
# completions: pg_prepared_statements table pg_catalog
# completions: ...
```

A half-written select list is read from its tokens.

```python
q = "--sql\nSELECT o.user_id, total, FROM <CURSOR> WHERE"
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

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "-- sql\nSELECT Total FROM <CURSOR>"
# completions: orders table -
# completions: user_names view -
# completions: users table -
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
q = "--sql\nSELECT email FROM <CURSOR>"
# completions: users table -
# completions: orders table -
# completions: user_names view -
```

### DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
q = "--sql\nSELECT id, name FROM u<CURSOR>"
# completions: user_names view -
# completions: users table -
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

## DuckDB table functions

```toml
[tool.sqlty]
dialect = "duckdb"
```

```python
q = "--sql\nSELECT * FROM read_cs<CURSOR>"
# completions: read_csv function read_csv(path: VARCHAR) -> TABLE (+2 more)
# completions: read_csv_auto function read_csv_auto(path: VARCHAR) -> TABLE (+2 more)
```
