# Deprecated columns

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.deprecated-columns]
"users.legacy_email" = { use = "email", remove-after = "2027-02-20" }
"public.orders.note" = {}
```

`schema.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT, legacy_email TEXT);
CREATE TABLE orders (id BIGINT PRIMARY KEY, user_id BIGINT, note TEXT);
```

## Reads anywhere in a query

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated: read `email` instead, it is removed after 2027-02-20"
by_old = SQL[tuple[int]]("--sql\nSELECT id FROM users WHERE legacy_email = %s")
```

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated"
# error: [deprecated-column] "`orders.note` is deprecated"
joined = SQL[tuple[int]]("--sql\nSELECT o.id FROM orders o JOIN users u ON u.legacy_email = o.note")
```

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated"
picked = SQL[tuple[str | None]]("--sql\nSELECT CASE WHEN id > 1 THEN legacy_email END FROM users")
```

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated"
returned = SQL[tuple[str | None]]("--sql\nUPDATE users SET email = %s WHERE id = %s RETURNING legacy_email")
```

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated"
copied = SQL("--sql\nUPDATE users SET email = legacy_email WHERE email IS NULL")
```

```python
from sqlty import SQL

# error: [deprecated-column] "`users.legacy_email` is deprecated"
nested = SQL[tuple[int]]("--sql\nSELECT id FROM orders WHERE user_id IN (SELECT id FROM users WHERE legacy_email IS NULL)")
```

## A column without `use` or `remove-after`

The schema of the key is dropped, as in `types`.

```python
from sqlty import SQL

# error: [deprecated-column] "`orders.note` is deprecated"
notes = SQL[tuple[str | None]]("--sql\nSELECT note FROM orders")
```

## Writes are not reported

```python
from sqlty import SQL

# error:
signup = SQL("--sql\nINSERT INTO users (id, email, legacy_email) VALUES (%s, %s, %s)")
sync = SQL("--sql\nUPDATE users SET email = %s, legacy_email = %s WHERE id = %s")
```

## A column of the same name elsewhere is not deprecated

```python
from sqlty import SQL

# error:
q = SQL[tuple[str | None]]("--sql\nWITH u AS (SELECT email AS legacy_email FROM users) SELECT legacy_email FROM u")
```

## `SELECT *` is not a read by name

```python
from sqlty import SQL

# error: [select-star]
q = SQL[tuple[int, str | None, str | None]]("--sql\nSELECT * FROM users")
```

## Rule severity

```toml
[tool.sqlty.rules]
deprecated-column = "ignore"
```

```python
from sqlty import SQL

# error:
q = SQL[tuple[str | None]]("--sql\nSELECT legacy_email FROM users")
```
