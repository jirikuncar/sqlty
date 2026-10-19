# String escapes

A query is checked as the string Python builds: escape sequences in the literal
are decoded before the query is analyzed, so `\"users\"` is the quoted name
`"users"`, and `\n` or `\t` are whitespace.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unparsed-query = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

## Decoded escapes

Every query below is typed, so `stale-type` reports its real row type.

```python
from sqlty import SQL

# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
a = SQL[tuple[int]]("--sql\nSELECT name FROM \"users\"")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
b = SQL[tuple[int]]('--sql\nSELECT \'x\'::TEXT AS b')
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
c = SQL[tuple[int]]("--sql\nSELECT id,\n\tname FROM users")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
d = SQL[tuple[int]]("--sql\nSELECT name FROM users WHERE name LIKE 'a\\%'")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
e = SQL[tuple[int]]("--sql\nSELECT \x6eame FROM users")
# error: [narrow-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
f = SQL[tuple[int]]("--sql\nSELECT name \
FROM users")
```

## Row types

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    name: int

# error: [column-type-mismatch] "User.name is int but query column 'name' infers str"
q = SQL[User]("--sql\nSELECT name FROM \"users\"")
```

## Raw strings are not checked

In a raw string, `\n` is a backslash and an `n`, so the marker comment runs to
the end of the query and nothing would reach the database.

```python
from sqlty import SQL

# error: [unchecked-query]
q = SQL[tuple[int]](r"--sql\nSELECT name FROM users")
```

## Rewrite keeps the escapes

```python
q = "--sql\nSELECT name FROM \"users\""
# rewritten: q = SQL[tuple[str]]("--sql\nSELECT name FROM \"users\"")
```
