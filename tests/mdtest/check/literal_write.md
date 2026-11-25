# Writing a value an enum or CHECK list does not allow

`literal-write` reports a constant written to a column whose values the schema lists.

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
    level INTEGER CHECK (level IN (1, 2)),
    kind pay_kind,
    note TEXT
);
```

## Every place a constant is written

```python
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
a = "--sql\nINSERT INTO orders (status) VALUES ('paid'), ('refunded')"
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
b = "--sql\nINSERT INTO orders VALUES (DEFAULT, 'refunded')"
# error: [literal-write] "`orders.level` is 1 or 2, not 3"
c = "--sql\nUPDATE orders SET level = 3 WHERE id = %s"
# error: [literal-write] "`orders.kind` is 'card' or 'iban', not 'cash'"
d = "--sql\nUPDATE orders SET kind = 'cash'::pay_kind WHERE id = %s"
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'it''s'"
e = "--sql\nINSERT INTO orders (id, status) VALUES (1, 'paid') ON CONFLICT (id) DO UPDATE SET status = 'it''s'"
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
# error: [literal-write] "`orders.level` is 1 or 2, not 3"
f = "--sql\nUPDATE orders SET (status, level) = ('refunded', 3) WHERE id = %s"
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
g = "--sql\nINSERT INTO orders (status) SELECT 'refunded'"
# error: [literal-write] "`orders.kind` is 'card' or 'iban', not 'cash'"
h = "--sql\nINSERT INTO orders (status, kind) SELECT 'paid', 'card' UNION ALL SELECT 'paid', 'cash'"
```

## What is not reported

A listed value, a parameter, an expression, `NULL`, a column without a list, a constant
of another kind (PostgreSQL reads `'3'` as an integer), a tuple `SET` from a subquery,
the rows `EXCEPT` removes and a `SELECT *` are not judged.

```python
# error:
a = "--sql\nINSERT INTO orders (status, level, kind) VALUES ('paid', 2, 'card')"
b = "--sql\nUPDATE orders SET status = %s, level = level + 1, note = 'x' WHERE id = %s"
c = "--sql\nUPDATE orders SET kind = NULL WHERE id = %s"
d = "--sql\nUPDATE orders SET level = '3', (status, note) = (SELECT 'refunded', 'x') WHERE id = %s"
e = "--sql\nINSERT INTO orders (status) SELECT 'paid' EXCEPT SELECT 'refunded'"
f = "--sql\nINSERT INTO orders SELECT * FROM orders"
```

## MySQL compares without case

```toml
[tool.sqlty]
dialect = "mysql"
schema = "mysql.sql"
```

`mysql.sql`:

```sql
CREATE TABLE orders (id INT PRIMARY KEY, status ENUM('pending', 'paid') NOT NULL);
```

```python
# error:
a = "-- sql\nUPDATE orders SET status = 'PAID' WHERE id = %s"
```

## Removing a value breaks the code that writes it

`sqlty breaking` checks the code that still writes `'refunded'` against the schema that
dropped it.

`schema.sql`:

```sql
CREATE TABLE orders (id SERIAL PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid', 'refunded')));
```

`against.sql`:

```sql
CREATE TABLE orders (id SERIAL PRIMARY KEY, status TEXT NOT NULL CHECK (status IN ('pending', 'paid')));
```

```python
# error: [literal-write] "`orders.status` is 'pending' or 'paid', not 'refunded'"
refund = "--sql\nUPDATE orders SET status = 'refunded' WHERE id = %s"
```
