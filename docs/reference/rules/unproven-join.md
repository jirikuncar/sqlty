# unproven-join

Default severity: **ignore**

## What it does

Reports a join in an aggregating query whose conditions match no primary or
unique key on either side, so sqlty cannot tell whether it repeats rows.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
unproven-join = "warn"
```

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER PRIMARY KEY, region TEXT NOT NULL, total NUMERIC NOT NULL);
CREATE TABLE targets (region TEXT NOT NULL, amount NUMERIC NOT NULL);
```

```python
import decimal

from sqlty import SQL

# error: [unproven-join] "the join of orders and targets matches no primary or unique key, so how often it repeats their rows is unknown"
query = SQL[tuple[decimal.Decimal | None]]("""--sql
SELECT SUM(o.total) FROM orders o JOIN targets t ON t.region = o.region""")
```

## Why is this bad?

[`fanout-aggregate`](fanout-aggregate.md) and [`chasm-join`](chasm-join.md)
only report what keys prove. When no key covers a join, a region with two
targets doubles its orders, and neither rule sees it.

## How to fix

Declare the key the data has: `UNIQUE (region)` on `targets`, or a unique
index. If the join may match several rows, aggregate that side first.

## Configuration

Off by default: in a schema that declares no keys, every join of an
aggregating query would be reported. Turn it on where the schema declares its
keys:

```toml
[tool.sqlty.rules]
unproven-join = "warn"
```
