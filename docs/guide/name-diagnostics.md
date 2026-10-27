# Name diagnostics

A query that reads a misspelled column, an unknown table or a function sqlty
does not know gets no type for that column, so its row type can't be checked.
Six rules explain why a column is untyped. The three that report SQL the
database rejects are errors by default; the three that report valid SQL sqlty
can't type are warnings.

| Rule | Default | Reports |
|------|---------|---------|
| [`unknown-table`](../reference/rules.md#unknown-table) | error | A table the schema does not define |
| [`unknown-column`](../reference/rules.md#unknown-column) | error | A column the table does not have |
| [`ambiguous-column`](../reference/rules.md#ambiguous-column) | error | An unqualified column in more than one table |
| [`unknown-function`](../reference/rules.md#unknown-function) | warn | A function sqlty cannot type |
| [`unknown-type`](../reference/rules.md#unknown-type) | warn | A cast or column type without a Python type |
| [`unsupported-syntax`](../reference/rules.md#unsupported-syntax) | warn | A construct that parses but is not typed |

## Change their severity

Set a rule in `[tool.sqlty.rules]` to `"error"` to fail `sqlty check`, `"warn"`
to print it, or `"ignore"` to turn it off. All rules but `unsupported-syntax` need a
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
# error: [unsupported-syntax] "expression `1 OPERATOR(pg_catalog.+) 1` is not typed"
f = SQL[tuple[object]]("--sql\nSELECT 1 OPERATOR(pg_catalog.+) 1")
```

To keep a rule on but accept one query, add `# sqlty: ignore[unknown-column]`
to its line. To turn a rule off again, set it to `"ignore"`.

## What is not reported

The rules resolve the names a query defines, so these are not false positives:

- CTE names, aliases (`FROM users u`, `AS t(a, b)`) and the columns of CTEs and
  subqueries in `FROM`.
- Columns of table functions sqlty does not read (an extension's or a user's
  function without a schema definition). A column taken from one is reported as
  `unsupported-syntax` instead of guessed. The built-in ones, such as
  `generate_series`, `unnest` and `json_each`, have known columns and are checked.
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
- Case: in PostgreSQL an unquoted name folds to lower case and a quoted name keeps
  its case, so `Users.ID` is `users.id` but `"ID"` is not `id`. A bare table name
  resolves through the `search_path` only. MySQL and SQLite match names without
  case.

The rules read the select list, `WHERE`, `JOIN ... ON`, `GROUP BY`, `HAVING` and
`ORDER BY` of a query, and the target table, set columns, values and conditions of
`INSERT`, `UPDATE` and `DELETE`, also when they return no rows. A name may also be a
result column's alias where the dialect reads one: SQLite anywhere, MySQL in
`GROUP BY`, `HAVING` and `ORDER BY`, PostgreSQL only as a bare name in `GROUP BY`
and `ORDER BY`.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from sqlty import SQL

a = SQL[tuple[str]]("--sql\nSELECT name AS n FROM users ORDER BY n")
# error: [unknown-column] "column `n` does not exist in `users`"
b = SQL[tuple[str]]("--sql\nSELECT name AS n FROM users ORDER BY lower(n)")
```

`NULL` and placeholders such as `$1` have no type by nature and are not
reported.
