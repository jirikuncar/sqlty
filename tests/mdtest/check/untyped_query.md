# Lint: untyped-query

`SQL[UNTYPED](...)` marks a query whose row type sqlty could not infer.
The `untyped-query` rule (default: warn) flags it until casts or a schema
make the row type known.

## SQL[UNTYPED] is flagged

```python
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

## Inferable query suggests the row type

```python
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; run sqlty check --fix"
q = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
```

## Rule can be ignored

```toml
[tool.sqlty.rules]
untyped-query = "ignore"
```

```python
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
# error:
```

## Module import style

```toml
[tool.sqlty]
import-style = "import sqlty"
[tool.sqlty.rules]
untyped-query = "warn"
```

```python
# error: [untyped-query] "sqlty.SQL[sqlty.UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
q = sqlty.SQL[sqlty.UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

## Sentinel: uninferable rows are stale until rewritten

With `untyped = "sentinel"`, rewrite wraps queries of unknown row type, so
check reports them and `--fix` wraps them.

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT x, y FROM pairs"
# fixed: from sqlty import SQL
# fixed: from sqlty import UNTYPED
# fixed: q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```
