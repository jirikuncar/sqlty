# Lint: untyped-query

`SQL[UNTYPED](...)` marks a query whose row type sqlty could not infer.
The `untyped-query` rule (default: warn) flags it until casts or a schema
make the row type known.

## SQL[UNTYPED] is flagged

```python
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

## Inferable query suggests the row type

```python
# error: [untyped-query] "SQL[UNTYPED] can now be SQL[tuple[int]]; strip and rewrite to update"
q = SQL[UNTYPED]("--sql\nSELECT id::INTEGER FROM users")
```

## Rule can be ignored

```toml
[tool.sqlty.rules]
untyped-query = "ignore"
```

```python
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
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
q = sqlty.SQL[sqlty.UNTYPED]("--sql\nVALUES (1, 2)")
```
