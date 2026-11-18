# foreign-key-mismatch

Default severity: **warn**

## What it does

Reports a foreign key column compared with a column it does not reference.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers);
CREATE TABLE warehouses (region TEXT, code TEXT, PRIMARY KEY (region, code));
CREATE TABLE stock (
    region TEXT NOT NULL,
    code TEXT NOT NULL,
    FOREIGN KEY (region, code) REFERENCES warehouses (region, code)
);
```

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "orders.customer_id references customers.id, not products.id"
query = SQL[tuple[str, int]]("""--sql
SELECT p.name, o.id
FROM orders o
JOIN products p ON p.id = o.customer_id""")
```

A foreign key column may be compared with:

- the column it references,
- the same column of another row, as in a self-join,
- another column referencing the same column.

The columns of a composite foreign key are compared with their referenced columns
of one row, all of them:

```python
from sqlty import SQL

# error: [foreign-key-mismatch] "stock (region, code) references warehouses (region, code), compared with w on region only"
query = SQL[tuple[str]]("""--sql
SELECT w.code
FROM stock s
JOIN warehouses w ON w.region = s.region""")
```

Comparisons are read in `ON`, `USING`, `WHERE` and `HAVING`: `=`, `<>`, row
values such as `(a, b) = (c, d)`, `IN` lists and `IN` subqueries. Each `OR`
branch and `IN` list item is judged on its own.

## Why is this bad?

Every id is an integer, so the query type-checks and returns rows: products
whose id happens to match a customer's. The mistake shows up as wrong data, not
as an error.

## How to fix

Compare the column with the one it references: `JOIN customers c ON c.id = o.customer_id`,
and every column of a composite key: `ON w.region = s.region AND w.code = s.code`.

## Configuration

Needs a [schema](../../../docs/guide/schema-inference.md) with foreign keys. Every
declared key counts, `NOT VALID` and `NOT ENFORCED` ones included; MySQL tables of
an engine other than InnoDB have none. Columns of views, CTEs and derived tables
are not judged. [`entity-mismatch`](entity-mismatch.md) also follows chains of
foreign keys and compares primary keys; both may report the same comparison.
