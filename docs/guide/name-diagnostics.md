# Name diagnostics

A query that reads a misspelled column, an unknown table or a function sqlty
does not know gets no type for that column, and `sqlty check` says nothing:
the check is skipped. Six rules explain why a column is untyped. They are off
by default, because they report code that passes today.

| Rule | Reports |
|------|---------|
| [`unknown-table`](../reference/rules.md#unknown-table) | A table the schema does not define |
| [`unknown-column`](../reference/rules.md#unknown-column) | A column the table does not have |
| [`ambiguous-column`](../reference/rules.md#ambiguous-column) | An unqualified column in more than one table |
| [`unknown-function`](../reference/rules.md#unknown-function) | A function sqlty cannot type |
| [`unknown-type`](../reference/rules.md#unknown-type) | A cast or column type without a Python type |
| [`unsupported-syntax`](../reference/rules.md#unsupported-syntax) | A construct that parses but is not typed |

## Enable them

Set the rules you want in `[tool.sqlty.rules]`, to `"warn"` to print them or
`"error"` to fail `sqlty check`. All rules but `unsupported-syntax` need a
[schema](schema-inference.md): without one every name, function and type is unknown, so they stay
silent.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-table = "error"
unknown-column = "error"
ambiguous-column = "error"
unknown-function = "warn"
unknown-type = "warn"
unsupported-syntax = "warn"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total NUMERIC(10, 2));
```

```python
from sqlty import SQL

# error: [unknown-column] "column `email` does not exist in `users`"
a = SQL[tuple[int, str]]("--sql\nSELECT id, email FROM users")
# error: [unknown-table] "table `user` is not defined in the schema"
b = SQL[tuple[int]]("--sql\nSELECT id FROM user")
# error: [ambiguous-column] "column reference `id` is ambiguous"
c = SQL[tuple[int]]("--sql\nSELECT id FROM users u JOIN orders o ON o.user_id = u.id")
# error: [unknown-function] "function `my_udf` is not known to sqlty"
d = SQL[tuple[int]]("--sql\nSELECT my_udf(id) FROM users")
# error: [unknown-type] "type `mood` has no Python type"
e = SQL[tuple[str]]("--sql\nSELECT name::mood FROM users")
# error: [unsupported-syntax] "expression `DATE '2020-01-01'` is not typed"
f = SQL[tuple[object]]("--sql\nSELECT DATE '2020-01-01'")
```

To keep a rule on but accept one query, add `# sqlty: ignore[unknown-column]`
to its line. To turn a rule off again, set it to `"ignore"`.

## What is not reported

The rules resolve the names a query defines, so these are not false positives:

- CTE names, aliases (`FROM users u`, `AS t(a, b)`) and the columns of CTEs and
  subqueries in `FROM`.
- Table functions (`generate_series`, `unnest`). Their columns are not read, so a
  column taken from one is reported as `unsupported-syntax` instead of guessed.
- PostgreSQL system catalogs. The common ones (`pg_class`, `pg_attribute`,
  `pg_type`, `pg_namespace`, `pg_index`, `pg_constraint`, `pg_proc`,
  `information_schema.tables` and `.columns`, [the full list](schema-inference.md#migrations-schemas-and-functions))
  have typed columns; other `pg_catalog` / `information_schema` / `pg_*` relations are not
  read, with the same `unsupported-syntax` report. A table the schema defines
  always wins, whatever its name starts with.
- Functions and types the schema defines (`CREATE FUNCTION`, `CREATE TYPE`,
  `CREATE DOMAIN`).
- `LATERAL` subqueries and correlated subqueries, which read the outer query.
- Columns merged by `USING` or `NATURAL` joins.
- Case: `Users.ID` is `users.id`, as in the database. Quoted names are matched
  the same way.

The rules read the select list of the query, so a column named only in
`WHERE`, `ON`, `GROUP BY` or `ORDER BY` is not checked.

`NULL` and placeholders such as `$1` have no type by nature and are not
reported.
