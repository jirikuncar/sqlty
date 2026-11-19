# unknown-type

Default severity: **warn**

## What it does

Reports a type that has no Python type: the target of a cast (`x::mood`), or the declared type of a schema column (`geometry`). Enum and domain types declared in the schema are known.

A column the rule reports is untyped because of its type, not because sqlty
cannot type the expression: an unknown function is
[`unknown-function`](unknown-function.md), and another construct
[`unsupported-syntax`](unsupported-syntax.md).

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

### Built-in types

A built-in type sqlty does not map, such as a PostgreSQL range or geometric type,
is reported without a schema too, since no schema can declare it:

```toml
[tool.sqlty]
dialect = "postgres"

[tool.sqlty.rules]
unknown-type = "warn"
```

```python
from sqlty import SQL

# error: [unknown-type] "type `INT4RANGE` has no Python type"
q = SQL[tuple[object]]("--sql\nSELECT CAST('[1,2)' AS INT4RANGE) AS r")
```

## Why is this bad?

A column of an unmapped type is untyped, so its annotation is not checked.

## How to fix

Cast to a type sqlty maps: `shape::TEXT`. Declare the type in the schema file if it is an enum or domain. To accept one case, put `# sqlty: ignore[unknown-type]` on its line.

## Configuration

A warning by default: the SQL is valid, but sqlty cannot map the type to a Python type, so the query is
not fully checked. A warning tells you why without failing CI on sqlty's gaps. Make it
fail, or turn it off:

```toml
[tool.sqlty.rules]
unknown-type = "error"
```

Without a schema with tables only built-in types are checked: every enum and domain would be unknown. A cast to an enum is reported until the enum is declared in the schema.
