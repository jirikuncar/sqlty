---
order: 265
group: "names"
severity: "error"
fixable: false
summary: "INSERT or UPDATE writes a constant an enum or CHECK list does not allow"
origin: "SQL"
---

# literal-write

Default severity: **error**

## What it does

Reports a constant that an `INSERT ... VALUES`, an `UPDATE ... SET` or an
`ON CONFLICT DO UPDATE SET` writes to a column whose values the schema lists, as an
enum or a `CHECK (column IN (...))`, when the constant is not one of them.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TYPE pay_kind AS ENUM ('card', 'iban');
CREATE TABLE orders (
    id SERIAL PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('pending', 'paid')),
    kind pay_kind
);
```

```python
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
refund = "--sql\nUPDATE orders SET status = 'refunded' WHERE id = %s"
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
add = "--sql\nINSERT INTO orders (status) VALUES ('refunded')"
# error: [literal-write] "`orders.kind` is 'card' or 'iban', not 'cash'"
cash = "--sql\nINSERT INTO orders (status, kind) VALUES ('paid', 'cash')"
```

## Why is this bad?

The database rejects the row: an enum has no such label, and the `CHECK` fails. It
fails at run time, after the deploy. A migration that removes a value breaks every
query that still writes it, which `sqlty breaking` reports in the backward direction.

## How to fix

Write one of the listed values, or add the value to the enum or the `CHECK` first:

```python
# error:
refund = "--sql\nUPDATE orders SET status = 'paid' WHERE id = %s"
```

## Configuration

An error by default: the database rejects such SQL. Lower or turn it off:

```toml
[tool.sqlty.rules]
literal-write = "warn"
```

Needs a [schema](../../guide/schema-inference.md), and is judged in the `postgres`
and `sqlite` dialects, where comparison is exact. Only constants are checked: a
parameter, an expression or a value a query selects is not.
