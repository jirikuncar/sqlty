# MySQL ALTER TABLE

Schema files are often migrations, so MySQL `ALTER TABLE` statements are applied
in file order.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## ADD, DROP and RENAME COLUMN

`schema.sql`:

```sql
CREATE TABLE users (id INT NOT NULL PRIMARY KEY, full_name VARCHAR(50), legacy TEXT);
ALTER TABLE users ADD COLUMN age INT NOT NULL;
ALTER TABLE users ADD COLUMN email VARCHAR(100) AFTER id;
ALTER TABLE users ADD COLUMN rank INT FIRST;
ALTER TABLE users DROP COLUMN legacy;
ALTER TABLE users RENAME COLUMN full_name TO name;
```

```sql
SELECT * FROM users
-- columns: rank: int | None (INT)
-- columns: id: int (INT)
-- columns: email: str | None (VARCHAR(100))
-- columns: name: str | None (VARCHAR(50))
-- columns: age: int (INT)
```

## MODIFY

`MODIFY` replaces the column's type, nullability and `ENUM` labels.

`schema.sql`:

```sql
CREATE TABLE orders (id INT NOT NULL PRIMARY KEY, total INT, status ENUM('new', 'paid') NOT NULL);
ALTER TABLE orders MODIFY total DECIMAL(10, 2) NOT NULL;
ALTER TABLE orders MODIFY COLUMN status ENUM('new', 'paid', 'shipped');
```

```sql
SELECT total, status FROM orders
-- columns: total: decimal.Decimal (DECIMAL(10, 2))
-- columns: status: typing.Literal['new', 'paid', 'shipped'] | None (ENUM('NEW', 'PAID', 'SHIPPED'))
```

## MODIFY keeps a primary key NOT NULL

A primary key column stays `NOT NULL` whatever its new definition says, until
`DROP PRIMARY KEY`.

`schema.sql`:

```sql
CREATE TABLE events (id INT, code INT, note TEXT, PRIMARY KEY (id, code));
ALTER TABLE events MODIFY id BIGINT;
ALTER TABLE events CHANGE code event_code BIGINT;
CREATE TABLE tags (name VARCHAR(10) PRIMARY KEY);
ALTER TABLE tags MODIFY name VARCHAR(20);
```

```sql
SELECT id, event_code, name FROM events, tags
-- columns: id: int (BIGINT)
-- columns: event_code: int (BIGINT)
-- columns: name: str (VARCHAR(20))
```

## DROP PRIMARY KEY

`schema.sql`:

```sql
CREATE TABLE events (id INT PRIMARY KEY);
ALTER TABLE events DROP PRIMARY KEY;
ALTER TABLE events MODIFY id BIGINT;
```

```sql
SELECT id FROM events
-- columns: id: int | None (BIGINT)
```

## CHANGE

`CHANGE` renames the column too, and `FIRST` / `AFTER` move it.

`schema.sql`:

```sql
CREATE TABLE orders (id INT NOT NULL PRIMARY KEY, amount INT, note TEXT);
ALTER TABLE orders CHANGE amount total DECIMAL(10, 2) NOT NULL;
ALTER TABLE orders CHANGE COLUMN note note VARCHAR(20) NOT NULL FIRST;
```

```sql
SELECT * FROM orders
-- columns: note: str (VARCHAR(20))
-- columns: id: int (INT)
-- columns: total: decimal.Decimal (DECIMAL(10, 2))
```

## RENAME TO and DROP TABLE

`schema.sql`:

```sql
CREATE TABLE old_users (id INT NOT NULL);
ALTER TABLE old_users RENAME TO users;
CREATE TABLE scratch (id INT);
DROP TABLE scratch;
```

```sql
SELECT id FROM users
-- inferred: tuple[int]
```

```toml
[tool.sqlty.rules]
unknown-table = "error"
```

```python
from sqlty import SQL

# error: [unknown-table] "table `old_users` is not defined in the schema"
a = SQL[tuple[int]]("-- sql\nSELECT id FROM old_users")
# error: [unknown-table] "table `scratch` is not defined in the schema"
b = SQL[tuple[int]]("-- sql\nSELECT id FROM scratch")
```
