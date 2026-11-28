# Highlighting

`core.highlight_source` lists the SQL tokens to color in a file, and `sqlty server` sends
them as semantic tokens. A string is highlighted exactly when `sqlty check` checks it.
`# highlighted:` lists `kind text` per token, a line each, `kind` the semantic token type
and its modifiers (`function.defaultLibrary`).

## Marked string

The marker is a comment. Names the schema does not define stay uncolored, even those
sqlparser knows as keywords, such as `name` and `status`.

```python
q = "--sql\nSELECT name, status FROM users WHERE id = $1 AND total > 1.5"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter $1
# highlighted: keyword AND
# highlighted: operator >
# highlighted: number 1.5
```

## Wrapper, adjacent literals and escapes

A token split across literals is colored in each; an escape sequence is colored whole.

```python
from sqlty import SQL

q = SQL[tuple[str]](
    "--sql\n"
    "SELECT 'a\tb' "
    'FR' "OM users"
)
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: string 'a\tb'
# highlighted: keyword FR
# highlighted: keyword OM
```

## Triple-quoted string

A token over several lines, such as a block comment, is colored on each.

```python
q = """-- sql
SELECT id /* the
key */ FROM users"""
# highlighted: comment -- sql
# highlighted: keyword SELECT
# highlighted: comment /* the
# highlighted: comment key */
# highlighted: keyword FROM
```

## Annotated helper

A string passed to a parameter marked `Annotated[LiteralString, SQL]` is highlighted, with
no `--sql` marker. So are psycopg's `%s` and `%(name)s`.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T]) -> list[T]: ...

fetch_all("SELECT id FROM users WHERE name = %(name)s OR id = %s", tuple[int])
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter %(name)s
# highlighted: keyword OR
# highlighted: operator =
# highlighted: parameter %s
```

## Configured function

```toml
[[tool.sqlty.functions]]
import-path = "pkg.db.fetch_all"
query-param = "query"
```

`pkg/db.py`:

```python
from typing import LiteralString

def fetch_all(query: LiteralString) -> list[tuple[int]]: ...
```

```python
from pkg.db import fetch_all

fetch_all(query="SELECT 1")
# highlighted: keyword SELECT
# highlighted: number 1
```

## Dialect

MySQL reads `#` as a comment and `?` as a placeholder.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "--sql\nSELECT id FROM users WHERE id = ? # one"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter ?
# highlighted: comment # one
```

## Unchecked strings

A string without a marker, an f-string, a bytes string and a call to an unmarked function
stay Python strings.

```python
from sqlty import SQL

def run(query: str) -> None: ...

a = "SELECT 1"
b = f"--sql\nSELECT {a}"
c = b"--sql\nSELECT 1"
run("SELECT 1")
# highlighted: None
```

## Function names

A word directly followed by `(` is a function, also when qualified or quoted. A keyword that
takes parentheses stays a keyword, and so does a column named like a function.

### PostgreSQL

```python
q = "--sql\nSELECT count(*), coalesce(name, ''), count FROM users WHERE created < pg_catalog.now() AND id IN (1) AND EXISTS (SELECT 1)"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: function count
# highlighted: operator *
# highlighted: function coalesce
# highlighted: string ''
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator <
# highlighted: function now
# highlighted: keyword AND
# highlighted: keyword IN
# highlighted: number 1
# highlighted: keyword AND
# highlighted: keyword EXISTS
# highlighted: keyword SELECT
# highlighted: number 1
```

A table, a CTE, an alias or a type followed by `(` is not a function.

```python
q = "--sql\nWITH c(a) AS (SELECT CAST(1 AS numeric(10, 2))) INSERT INTO users(id) SELECT a::varchar(3) FROM c AS x(a)"
# highlighted: comment --sql
# highlighted: keyword WITH
# highlighted: variable.declaration c
# highlighted: keyword AS
# highlighted: keyword SELECT
# highlighted: keyword CAST
# highlighted: number 1
# highlighted: keyword AS
# highlighted: type numeric
# highlighted: number 10
# highlighted: number 2
# highlighted: keyword INSERT
# highlighted: keyword INTO
# highlighted: keyword SELECT
# highlighted: property a
# highlighted: operator ::
# highlighted: type varchar
# highlighted: number 3
# highlighted: keyword FROM
# highlighted: variable c
# highlighted: keyword AS
# highlighted: variable.declaration x
```

An alias without `AS`, and a later CTE, are not functions either.

```python
q = "--sql\nWITH a AS (SELECT 1), b(x) AS (SELECT 2) SELECT n FROM b, generate_series(1, 3) g(n)"
# highlighted: comment --sql
# highlighted: keyword WITH
# highlighted: variable.declaration a
# highlighted: keyword AS
# highlighted: keyword SELECT
# highlighted: number 1
# highlighted: variable.declaration b
# highlighted: keyword AS
# highlighted: keyword SELECT
# highlighted: number 2
# highlighted: keyword SELECT
# highlighted: property n
# highlighted: keyword FROM
# highlighted: variable b
# highlighted: function generate_series
# highlighted: number 1
# highlighted: number 3
# highlighted: variable.declaration g
```

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
q = "--sql\nSELECT count(*), `my fn`(1), count FROM users WHERE created < now() AND id IN (1)"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: function count
# highlighted: operator *
# highlighted: function `my fn`
# highlighted: number 1
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator <
# highlighted: function now
# highlighted: keyword AND
# highlighted: keyword IN
# highlighted: number 1
```

### SQLite

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
q = "--sql\nSELECT count(*), ifnull(name, ''), count FROM users WHERE id IN (VALUES (1))"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: function count
# highlighted: operator *
# highlighted: function ifnull
# highlighted: string ''
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: keyword IN
# highlighted: keyword VALUES
# highlighted: number 1
```

## Names

A name is colored by what it resolves to, as `sqlty check` reads it: a table, a view, the
schema that qualifies them, an alias or CTE (marked where the query defines it) and a
column (marked when the project deprecates it). A cast's type and a function are built in
unless the schema defines them. A name that resolves to nothing stays uncolored.

### PostgreSQL

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.deprecated-columns]
"users.legacy_email" = { use = "email" }
```

`schema.sql`:

```sql
CREATE TABLE users (id INT, name TEXT, users INT, legacy_email TEXT);
CREATE TABLE orders (user_id INT, total NUMERIC);
CREATE VIEW active AS SELECT id FROM users;
CREATE TYPE mood AS ENUM ('happy', 'sad');
CREATE FUNCTION score(x INT) RETURNS INT LANGUAGE sql AS 'SELECT 1';
```

```python
q = """--sql
WITH big AS (SELECT user_id, SUM(total) AS spent FROM public.orders GROUP BY user_id)
SELECT u.name, big.spent::numeric FROM users AS u JOIN big ON big.user_id = u.id
"""
# highlighted: comment --sql
# highlighted: keyword WITH
# highlighted: variable.declaration big
# highlighted: keyword AS
# highlighted: keyword SELECT
# highlighted: property user_id
# highlighted: function.defaultLibrary SUM
# highlighted: property total
# highlighted: keyword AS
# highlighted: keyword FROM
# highlighted: namespace public
# highlighted: class orders
# highlighted: keyword GROUP
# highlighted: keyword BY
# highlighted: property user_id
# highlighted: keyword SELECT
# highlighted: variable u
# highlighted: property name
# highlighted: variable big
# highlighted: property spent
# highlighted: operator ::
# highlighted: type.defaultLibrary numeric
# highlighted: keyword FROM
# highlighted: class users
# highlighted: keyword AS
# highlighted: variable.declaration u
# highlighted: keyword JOIN
# highlighted: variable big
# highlighted: keyword ON
# highlighted: variable big
# highlighted: property user_id
# highlighted: operator =
# highlighted: variable u
# highlighted: property id
```

A column named like its table is a column where a column is read, and the table where a
relation is.

```python
q = "--sql\nSELECT users, score(id), name::mood, legacy_email FROM users WHERE users.id IN (SELECT id FROM active) AND missing = 1"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: property users
# highlighted: function score
# highlighted: property id
# highlighted: property name
# highlighted: operator ::
# highlighted: type mood
# highlighted: property.deprecated legacy_email
# highlighted: keyword FROM
# highlighted: class users
# highlighted: keyword WHERE
# highlighted: class users
# highlighted: property id
# highlighted: keyword IN
# highlighted: keyword SELECT
# highlighted: property id
# highlighted: keyword FROM
# highlighted: interface active
# highlighted: keyword AND
# highlighted: operator =
# highlighted: number 1
```

The columns an `INSERT` lists and an `UPDATE` sets are columns of their table.

```python
a = "--sql\nINSERT INTO users (id, name) VALUES (1, 'x')"
b = "--sql\nUPDATE users AS u SET name = 'x' WHERE u.id = 1"
# highlighted: comment --sql
# highlighted: keyword INSERT
# highlighted: keyword INTO
# highlighted: class users
# highlighted: property id
# highlighted: property name
# highlighted: keyword VALUES
# highlighted: number 1
# highlighted: string 'x'
# highlighted: comment --sql
# highlighted: keyword UPDATE
# highlighted: class users
# highlighted: keyword AS
# highlighted: variable.declaration u
# highlighted: keyword SET
# highlighted: property name
# highlighted: operator =
# highlighted: string 'x'
# highlighted: keyword WHERE
# highlighted: variable u
# highlighted: property id
# highlighted: operator =
# highlighted: number 1
```

A query being typed keeps the colors its tokens give.

```python
q = "--sql\nSELECT count(id), name::text FROM users WHERE"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: function.defaultLibrary count
# highlighted: operator ::
# highlighted: type.defaultLibrary text
# highlighted: keyword FROM
# highlighted: keyword WHERE
```

### MySQL

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INT, name TEXT);
```

```python
q = "--sql\nSELECT u.`name`, CAST(u.id AS CHAR) FROM users AS u WHERE id = ?"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: variable u
# highlighted: property `name`
# highlighted: keyword CAST
# highlighted: variable u
# highlighted: property id
# highlighted: keyword AS
# highlighted: type.defaultLibrary CHAR
# highlighted: keyword FROM
# highlighted: class users
# highlighted: keyword AS
# highlighted: variable.declaration u
# highlighted: keyword WHERE
# highlighted: property id
# highlighted: operator =
# highlighted: parameter ?
```

### SQLite

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
q = "--sql\nWITH named AS (SELECT id, name FROM users) SELECT n.name FROM named n WHERE n.id = ?"
# highlighted: comment --sql
# highlighted: keyword WITH
# highlighted: variable.declaration named
# highlighted: keyword AS
# highlighted: keyword SELECT
# highlighted: property id
# highlighted: property name
# highlighted: keyword FROM
# highlighted: class users
# highlighted: keyword SELECT
# highlighted: variable n
# highlighted: property name
# highlighted: keyword FROM
# highlighted: variable named
# highlighted: variable.declaration n
# highlighted: keyword WHERE
# highlighted: variable n
# highlighted: property id
# highlighted: operator =
# highlighted: parameter ?
```

### DuckDB

```toml
[tool.sqlty]
dialect = "duckdb"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name VARCHAR);
```

```python
q = "--sql\nSELECT name::VARCHAR, u.id FROM main.users u"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: property name
# highlighted: operator ::
# highlighted: type.defaultLibrary VARCHAR
# highlighted: variable u
# highlighted: property id
# highlighted: keyword FROM
# highlighted: namespace main
# highlighted: class users
# highlighted: variable.declaration u
```
