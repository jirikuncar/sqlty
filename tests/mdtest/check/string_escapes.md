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

## Every escape is decoded before the query is read

A query is typed only when its escapes decode to valid SQL: `\x31`, `\u0031`,
`\U00000031` and the octal `\061` are all the digit `1`, and a backslash before a
line break, in any of its spellings, joins the lines.

```python
a = "--sql\nSELECT \x31::INTEGER"
b = "--sql\nSELECT \u0031::INTEGER"
c = "--sql\nSELECT \U00000031::INTEGER"
d = "--sql\nSELECT \061::INTEGER"
e = "--sql\nSELECT\tid\r\n,\
 1::INTEGER FROM users"
f = "--sql\nSELECT '\d'::TEXT"
g = "--sql\nSELECT '\a\b\f\v\r\n'::TEXT"
# rewritten: a = SQL[tuple[int]]("--sql\nSELECT \x31::INTEGER")
# rewritten: b = SQL[tuple[int]]("--sql\nSELECT \u0031::INTEGER")
# rewritten: c = SQL[tuple[int]]("--sql\nSELECT \U00000031::INTEGER")
# rewritten: d = SQL[tuple[int]]("--sql\nSELECT \061::INTEGER")
# rewritten: e = SQL[tuple[int, int]]("--sql\nSELECT\tid\r\n,\
# rewritten:  1::INTEGER FROM users")
# rewritten: f = SQL[tuple[str]]("--sql\nSELECT '\d'::TEXT")
# rewritten: g = SQL[tuple[str]]("--sql\nSELECT '\a\b\f\v\r\n'::TEXT")
```

## Columns count escapes as written

The range of a diagnostic is in the source: an escape before it is as long as it is
written.

```python
from sqlty import SQL

# error[45:49]: [unknown-column] "column `nope` does not exist in `users`"
a = SQL[tuple[str]]("--sql\nSELECT \x6eame, nope FROM users")
# error[47:51]: [unknown-column] "column `nope` does not exist in `users`"
b = SQL[tuple[str]]("--sql\nSELECT \u006eame, nope FROM users")
# error[43:47]: [unknown-column] "column `nope` does not exist in `users`"
c = SQL[tuple[str]]("--sql\nSELECT name,\tnope FROM users")
# error[45:49]: [unknown-column] "column `nope` does not exist in `users`"
d = SQL[tuple[str]]("--sql\nSELECT na\x6de, nope" "\n FROM users")
# error[48:64]: [unknown-column] "column `nope` does not exist in `users`"
e = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER, \x6e\x6f\x70\x65 FROM users")
```
