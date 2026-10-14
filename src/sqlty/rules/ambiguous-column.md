# ambiguous-column

Default severity: **ignore**

## What it does

Reports an unqualified column that more than one table of the query has.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
ambiguous-column = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
CREATE TABLE orders (id INTEGER, user_id INTEGER);
```

```python
from sqlty import SQL

# error: [ambiguous-column] "column reference `id` is ambiguous"
q = SQL[tuple[int]]("--sql\nSELECT id FROM users u JOIN orders o ON o.user_id = u.id")
```

## Why is this bad?

The database rejects the query. sqlty types the column from the first table that has it, so the annotation can look right while the query fails.

## How to fix

Qualify the column: `u.id`. A column merged by `USING (id)` or any column of a `NATURAL` join is single and not reported; a subquery's own tables shadow the outer query. To accept one case, put `# sqlty: ignore[ambiguous-column]` on its line.

## Configuration

Off by default, because it reports code that passes today. Enable it:

```toml
[tool.sqlty.rules]
ambiguous-column = "warn"
```

Only checked when a schema with tables is configured. Applies to the `postgres`, `mysql` and `sqlite` dialects. `error` makes `sqlty check` fail on it.
