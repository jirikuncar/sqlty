# unknown-table

Default severity: **ignore**

## What it does

Reports a query that reads a table the configured schema does not define: a `FROM` or `JOIN` item, a DML target with `RETURNING`, or a qualifier (`x.id`, `x.*`) that names no table or alias in the query.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from sqlty import SQL

# error: [unknown-table] "table `user` is not defined in the schema"
q = SQL[tuple[int]]("--sql\nSELECT id FROM user")
```

## Why is this bad?

A column of an unknown table has no type, so the annotation of the row is not checked. A typo in a table name, or a table missing from the schema dump, hides every column below it.

## How to fix

Fix the name, or add the table to the schema file. CTEs, derived tables, views in the schema, table functions (`generate_series`, `unnest`), system catalogs (`pg_catalog`, `information_schema`, `pg_*`) and DML `FROM`/`USING` sources are not reported. To accept one case, put `# sqlty: ignore[unknown-table]` on its line.

## Configuration

Off by default, because it reports code that passes today. Enable it:

```toml
[tool.sqlty.rules]
unknown-table = "warn"
```

Only checked when a schema with tables is configured: without one every table is unknown, so the rule stays silent. Applies to the `postgres`, `mysql` and `sqlite` dialects. Case is folded as the database does, so `Users` and `users` are one table. `error` makes `sqlty check` fail on it.
