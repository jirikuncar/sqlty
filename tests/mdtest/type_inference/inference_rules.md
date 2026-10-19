# Inference rules behind a column

`-- rules:` shows, per result column, the inference rule that gave its type and
the one that decided its nullability (`-` for none), as `sqlty check --explain`
prints them.

`schema.sql`:

```sql
CREATE TABLE orders (id INTEGER NOT NULL, qty SMALLINT NOT NULL, price NUMERIC(10, 2), doc JSONB, placed DATE NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Operators and functions

A column gets the rules of its outermost operator or function call.

```sql
SELECT qty + 1 AS next, SUM(qty) AS total, COUNT(*) AS n, doc -> 'a' AS a, placed + 7 AS due FROM orders GROUP BY qty, doc, placed
-- rules: next: common.arithmetic.numbers / common.arithmetic.numbers
-- rules: total: postgres.aggregate.sum-integers / postgres.aggregate.sum-integers
-- rules: n: common.aggregate.count / common.aggregate.count
-- rules: a: postgres.json.arrow-jsonb / postgres.json.arrow-jsonb
-- rules: due: postgres.datetime.date-days / postgres.datetime.date-days
```

## Columns without a rule

A column reference, a cast, a literal and a function no rule covers have none.
A star expands to columns without rules.

```sql
SELECT id, price::TEXT AS p, 'x' AS x, gcd(id, 6) AS g, * FROM orders
-- rules: id: - / -
-- rules: p: - / -
-- rules: x: - / -
-- rules: g: - / -
-- rules: id: - / -
-- rules: qty: - / -
-- rules: price: - / -
-- rules: doc: - / -
-- rules: placed: - / -
```

## Set operations

A set operation unifies its branches' columns, so no single rule types them.

```sql
SELECT qty + 1 AS v FROM orders UNION SELECT 2
-- rules: v: - / -
```

## RETURNING

```sql
UPDATE orders SET qty = qty + 1 RETURNING qty * 2 AS doubled, id
-- rules: doubled: common.arithmetic.numbers / common.arithmetic.numbers
-- rules: id: - / -
```

## MySQL interval arithmetic

`date + INTERVAL n unit` is typed by its unit in code, not by a rule.

```toml
[tool.sqlty]
dialect = "mysql"
```

```sql
SELECT CURRENT_DATE + INTERVAL 1 DAY AS tomorrow, 1 + 2 AS three
-- rules: tomorrow: - / -
-- rules: three: common.arithmetic.numbers / common.arithmetic.numbers
```
