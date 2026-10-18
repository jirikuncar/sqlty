# Lint: untyped-query

`SQL[UNTYPED](...)` marks a query whose row type sqlty could not infer.
The `untyped-query` rule (default: warn) flags it until casts or a schema
make the row type known.

## SQL[UNTYPED] is flagged

```python
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type: columns `x`, `y` are untyped; add casts (for example id::INTEGER) or a schema"
q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

## The cause is named

With a schema, the message names what could not be typed and suggests the fix
for it, not a schema.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER NOT NULL, email TEXT NOT NULL, prefs JSONB);
```

```python
# error: [unknown-function] "function `mystery_fn` is not known to sqlty, so its result is untyped"
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type: function `mystery_fn` is not known to sqlty, so its result is untyped; cast its result, or declare the function in the schema"
a = SQL[UNTYPED]("--sql\nSELECT id, mystery_fn(email) FROM users")
# error: [unsupported-syntax] "expression `1 OPERATOR(pg_catalog.+) 1` is not typed"
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type: expression `1 OPERATOR(pg_catalog.+) 1` is not typed; cast it to a type"
b = SQL[UNTYPED]("--sql\nSELECT id, 1 OPERATOR(pg_catalog.+) 1 FROM users")
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type: column `nothing` is untyped; cast it to a type"
c = SQL[UNTYPED]("--sql\nSELECT id, NULL AS nothing FROM users")
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
# error: [untyped-query] "sqlty.SQL[sqlty.UNTYPED] has no inferable row type: columns `x`, `y` are untyped; add casts (for example id::INTEGER) or a schema"
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
