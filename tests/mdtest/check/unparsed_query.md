# Lint: unparsed-query

A query that does not parse gets no row type, so nothing checks its annotation.
The `unparsed-query` rule (default: ignore) reports it and quotes sqlparser's
error. `xmlelement(name foo)` is valid PostgreSQL that sqlparser does not read.

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
q = "--sql\nSELECT xmlelement(name foo)"
```

## A marked string that does not parse

```python
# error: [unparsed-query] "query does not parse: Expected: ), found: foo"
q = "--sql\nSELECT xmlelement(name foo)"
```

## A wrapped query that does not parse

```python
from sqlty import SQL

# error: [unparsed-query] "query does not parse: Expected: ), found: foo"
q = SQL[tuple[str]]("--sql\nSELECT xmlelement(name foo)")
```

## A query passed to an annotated helper

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_value[T](query: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...

# error: [unparsed-query] "query does not parse: Expected: ), found: foo"
n = fetch_value("SELECT xmlelement(name foo)", str)
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
