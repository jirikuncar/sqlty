# unknown-database

Default severity: **error**

## What it does

Flags a query whose marker names a database the project does not configure under
[`databases`](../../../docs/reference/configuration.md#databases).

## Why is this bad?

The query falls back to the file's database, so its row type is checked against the
wrong schema and dialect.

## Example

```toml
[tool.sqlty.databases.analytics]
dialect = "sqlite"
```

```python
from sqlty import SQL

# error: [unknown-database] "database `analytic` is not configured; did you mean `analytics`?"
q = SQL[tuple[int]]("--sql sqlty:db=analytic\nSELECT 1::INTEGER")
```

Fix the name, or add the database to the configuration. `default` names the top-level
database. In a `.sql` file the rule reports the `-- sqlty: db=NAME` header line.
