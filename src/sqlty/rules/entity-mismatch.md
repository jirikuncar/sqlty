# entity-mismatch

Default severity: **ignore**

## What it does

Reports a comparison between two columns that hold keys of different tables.
Turn it on in `[tool.sqlty.rules]`:

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
entity-mismatch = "warn"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers);
```

```python
from sqlty import SQL

# error: [entity-mismatch] "products.id holds products keys, orders.customer_id holds customers keys"
query = SQL[tuple[str, int]]("""--sql
SELECT p.name, o.id
FROM orders o
JOIN products p ON p.id = o.customer_id""")
```

A primary or unique key column holds keys of its table. A foreign key column
holds keys of the column it references, followed to the end of the chain, so a
column referencing `orders.customer_id` holds `customers` keys too. Columns
that are no key and reference none are not judged.

Comparisons are read in `ON`, `USING`, `WHERE` and `HAVING`, `=` and `<>`, `IN`
lists, and `IN` subqueries that select one column.

## Why is this bad?

Every id is an integer, so the query type-checks and returns rows: products
whose id happens to match a customer's. The mistake shows up as wrong data, not
as an error.

## How to fix

Join on the key the column references: `JOIN customers c ON c.id = o.customer_id`.

## Configuration

Off by default. Joins between unrelated keys can be intentional, such as a
polymorphic `owner_id` or a legacy schema without foreign keys, and existing
code would need suppressions. Enable it per project:

```toml
[tool.sqlty.rules]
entity-mismatch = "warn"
```

Needs a [schema](../../../docs/guide/schema-inference.md) with primary keys and
foreign keys. Views, CTEs and derived tables don't carry entities.
