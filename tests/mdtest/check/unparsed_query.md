# Lint: unparsed-query

A query that does not parse gets no row type, so nothing checks its annotation.
The `unparsed-query` rule (default: ignore) reports it and quotes sqlparser's
error. `NULL::xml IS DOCUMENT` is valid PostgreSQL that sqlparser does not read.

```toml
[tool.sqlty]
dialect = "postgres"

[tool.sqlty.rules]
unparsed-query = "warn"
```

## Silent when the rule is ignored

```toml
[tool.sqlty.rules]
unparsed-query = "ignore"
```

```python
q = "--sql\nSELECT NULL::xml IS DOCUMENT"
```

## A marked string that does not parse

```python
# error: [unparsed-query] "found: DOCUMENT"
q = "--sql\nSELECT NULL::xml IS DOCUMENT"
```

## A wrapped query that does not parse

```python
from sqlty import SQL

# error: [unparsed-query] "found: DOCUMENT"
q = SQL[tuple[str]]("--sql\nSELECT NULL::xml IS DOCUMENT")
```

## A query passed to an annotated helper

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_value[T](query: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...

# error: [unparsed-query] "found: DOCUMENT"
n = fetch_value("SELECT NULL::xml IS DOCUMENT", str)
```

## Queries that parse are not reported

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users FOR NO KEY UPDATE")
```

PostgreSQL's row-lock clauses, `SET CONSTRAINTS` and `DO` blocks, which sqlparser does not
read, are read as the closest syntax it does. A `DO` block's code is not read.

```python
# error:
a = "--sql\nSET CONSTRAINTS ALL DEFERRED"
b = "--sql\nSET CONSTRAINTS orders_user_id_fkey, public.items_order_id_fkey IMMEDIATE"
c = "--sql\n-- defer FK checks\nSET CONSTRAINTS ALL DEFERRED"
d = "--sql\nBEGIN; /* defer */ SET CONSTRAINTS \"orders_fk\" DEFERRED; COMMIT"
e = "--sql\nDO $$ BEGIN PERFORM 1; END $$"
f = "--sql\nDO LANGUAGE plpgsql $body$ BEGIN RAISE NOTICE '$$'; END $body$"
g = "--sql\nDO 'BEGIN PERFORM 1; END' LANGUAGE plpgsql"
```

## Syntax of another dialect

Each dialect's own syntax that sqlparser does not read, such as MySQL's infix
`MOD` or SQLite's `a IS b`, is read only in that dialect. In PostgreSQL both are
syntax errors.

```python
# error: [unparsed-query] "found: 2"
a = "--sql\nSELECT 5 MOD 2"
# error: [unparsed-query] "found: 2"
b = "--sql\nSELECT 5 IS 2"
```

## Syntax the dialect lacks

SQLite has no `SELECT ... INTO`; sqlparser reads it in every dialect.

```toml
[tool.sqlty]
dialect = "sqlite"
```

```python
# error: [unparsed-query] "query does not parse: unsupported in this dialect"
q = "--sql\nSELECT id INTO backup FROM users"
```
