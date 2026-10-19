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

PostgreSQL's row-lock clauses and `SET CONSTRAINTS`, which sqlparser does not read, are
read as the closest syntax it does.

```python
# error:
a = "--sql\nSET CONSTRAINTS ALL DEFERRED"
b = "--sql\nSET CONSTRAINTS orders_user_id_fkey, public.items_order_id_fkey IMMEDIATE"
```
