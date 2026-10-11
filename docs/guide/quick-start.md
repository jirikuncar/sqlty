# Quick Start

Mark SQL strings with `--sql\n`, declare column types with `::TYPE` casts, and
let `sqlty rewrite` add the row type. Runnable project:
[`examples/01_quick_start/`](../../examples/01_quick_start/).

## Write queries

The `--sql` marker is a SQL comment, so the query itself starts on the next
line. Use `\n` in single-line strings and a real newline in triple-quoted ones:

```python
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = """--sql
SELECT COUNT(*)::INTEGER FROM users"""
```

## Rewrite

`sqlty rewrite .` wraps every query in `SQL[T](...)`, where `T` is the row
type, and adds the import. String content is never modified:

```python
user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
count_query = """--sql
SELECT COUNT(*)::INTEGER FROM users"""
# rewritten-file: from sqlty import SQL
# rewritten-file: user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# rewritten-file: count_query = SQL[tuple[int]]("""--sql
# rewritten-file: SELECT COUNT(*)::INTEGER FROM users""")
```

`SQL[T]` is a `str` subclass: it works wherever a string works, and editors,
type checkers and database drivers see the row type.

## Check

`sqlty check .` reports queries that are not wrapped yet, row types that no
longer match the SQL, and the rest of the [lint rules](../rules.md):

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
user_query = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"
```

## Strip

`sqlty strip .` is the exact inverse of rewrite. Strip before editing a query,
then rewrite again:

```python
from sqlty import SQL

user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# stripped: user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
```

## The space pitfall

`"--sql SELECT ..."` with a space turns the whole line into a SQL comment:
the query is empty, so nothing is inferred.

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT id::INTEGER FROM users"
# rewritten: q = "--sql SELECT id::INTEGER FROM users"
```

## Commands

```bash
sqlty rewrite .           # "--sql\n..." -> SQL[T]("--sql\n...")
sqlty rewrite --check .   # exit 1 if files need rewriting (CI)
sqlty rewrite --diff .    # show the diff, change nothing
sqlty check .             # lint; exit 1 on errors
sqlty check --fix .       # rewrite, refresh stale types, expand SELECT *
sqlty strip .             # SQL[T]("--sql\n...") -> "--sql\n..."
```

Next: [schema inference](schema-inference.md) drops the casts,
[user types](user-types.md) replace `tuple[...]` with named rows.
