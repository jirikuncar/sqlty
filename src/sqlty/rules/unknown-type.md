# unknown-type

Default severity: **ignore**

## What it does

Reports a type that has no Python type: the target of a cast (`x::mood`), or the declared type of a schema column (`geometry`). Enum and domain types declared in the schema are known.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-type = "warn"
```

`schema.sql`:

```sql
CREATE TABLE places (id INTEGER, shape GEOMETRY);
```

```python
from sqlty import SQL

# error: [unknown-type] "column `places.shape` has type `GEOMETRY`"
q = SQL[tuple[int, str]]("--sql\nSELECT id, shape FROM places")
```

## Why is this bad?

A column of an unmapped type is untyped, so its annotation is not checked.

## How to fix

Cast to a type sqlty maps: `shape::TEXT`. Declare the type in the schema file if it is an enum or domain. To accept one case, put `# sqlty: ignore[unknown-type]` on its line.

## Configuration

Off by default, because it reports code that passes today. Enable it:

```toml
[tool.sqlty.rules]
unknown-type = "warn"
```

A cast does not need a schema to be reported, so a cast to an enum is reported until the enum is declared in one. `error` makes `sqlty check` fail on it.
