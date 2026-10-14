# Lint Rule Configuration

## Default config — star without schema is an error

```python
from sqlty import SQL
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
```

## All rules disabled — no diagnostics

```toml
[tool.sqlty.rules]
select-star = "ignore"
select-star-no-schema = "ignore"
```

```python
from sqlty import SQL
q = SQL[tuple[str]]("--sql\nSELECT * FROM users")
# error:
```

## Star rule set to error

`schema.sql`:
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
```

```toml
[tool.sqlty]
schema = "schema.sql"
[tool.sqlty.rules]
select-star = "error"
```

```python
from sqlty import SQL
# error: [select-star] "SELECT * used — expand to explicit column list (joins need a manual edit)"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

## Uninferable wrapped query is not stale

A `--sql SELECT ...` space form is a SQL comment with no row type; only
`inline-sql-comment` reports it.

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
```

## String after an f-string is reported as stale

```python
table = "users"
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
f = f"--sql\nSELECT id FROM {table}"
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT id::INTEGER FROM users"
```

## Space after the marker in prose is not reported

`inline-sql-comment` needs a statement after the marker, as `check --fix` does.

```python
msg = "--sql flag enables it"
todo = "--sql delete me later"
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql DELETE FROM users"
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
r = u"--sql select id::INTEGER from users"
```

## Triple-quoted wrappers are checked

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[typing.Literal[1]]]"
a = SQL[tuple[str]]("--sql\nSELECT 1 AS n")
# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[typing.Literal[1]]]"
b = SQL[tuple[str]]("""--sql
SELECT 1 AS n""")
# error: [stale-type] "SQL[tuple[str]] should be SQL[tuple[typing.Literal[1]]]"
c = SQL[tuple[str]]("""--sql\nSELECT 1 AS n""")
```

## Unwrapped string with an escaped CRLF marker

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
a = "--sql\r\nSELECT 1 AS n"
```

## Unwrapped triple-quoted string is stale

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = """--sql
SELECT 1 AS n"""
```

## Triple-quoted wrappers get select-star and untyped-query

```python
from sqlty import SQL, UNTYPED

# error: [select-star-no-schema] "SELECT * used but no schema available"
a = SQL[tuple[str]]("""--sql
SELECT * FROM users""")
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[typing.Literal[1]]]; run sqlty check --fix"
b = SQL[UNTYPED]("""-- sql
SELECT 1 AS n""")
```

## Escaped CRLF marker with the sentinel

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
q = "--sql\r\nSELECT x, y FROM pairs"
# rewritten: q = SQL[UNTYPED]("--sql\r\nSELECT x, y FROM pairs")
```

## Marker on the second line of a triple-quoted string

`check` reports exactly the strings `rewrite` wraps.

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = """
--sql
SELECT 1::INTEGER AS n"""
# rewritten: q = SQL[tuple[int]]("""
# rewritten: --sql
# rewritten: SELECT 1::INTEGER AS n""")
```

## Wrapped and unwrapped strings on one line

```python
from sqlty import SQL
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
a, b = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER"), "--sql\nSELECT 2::INTEGER"
```

## Concatenated and `u` strings are reported on their first line

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = ("--sql\nSELECT 1::INTEGER "
     "AS n")
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
r = u"--sql\nSELECT 2::INTEGER AS m"
```
