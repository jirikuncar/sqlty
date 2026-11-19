# duplicate-alias

Default severity: **error**

## What it does

Reports two tables, subqueries or functions of one `FROM` clause that are read
by the same name.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL, amount NUMERIC NOT NULL);
```

```python
from sqlty import SQL

# error: [duplicate-alias] "table name `p` is specified more than once"
q = SQL("--sql\nSELECT p.name, p.amount FROM products p JOIN payments p ON p.order_id = p.id")
# error: [duplicate-alias] "table name `products` is specified more than once"
r = SQL[tuple[int]]("--sql\nSELECT 1 FROM products JOIN products ON true")
```

## Why is this bad?

PostgreSQL rejects the query with `table name "p" specified more than once`,
MySQL with `Not unique table/alias: 'p'`. The columns of the query can't be
told apart, so sqlty doesn't report them as unknown either.

## How to fix

Give each `FROM` item its own alias: `FROM products pr JOIN payments pa`.

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
duplicate-alias = "warn"
```

Needs no schema. Checked in the `postgres` and `mysql` dialects; SQLite accepts
the query. Two tables without an alias may share a name when they are in
different schemas: `FROM public.users, audit.users`.
