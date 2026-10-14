# User type names outside PostgreSQL

MySQL and SQLite have no schemas, so a type is found by its last name part. A quoted
name that contains a dot is one name: it does not stand for the part after the dot.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## A qualified name finds the type

`schema.sql`:

```sql
CREATE TYPE mood AS ENUM ('a', 'b');
CREATE TABLE t (id INT NOT NULL, m db.mood);
```

```sql
SELECT id, m FROM t
-- inferred: tuple[int, typing.Literal['a', 'b'] | None]
```

## A quoted dotted name is not its last part

`x.y` is a different type from `y`, so the column is not typed from `y`.

`schema.sql`:

```sql
CREATE TYPE y AS ENUM ('a', 'b');
CREATE TABLE t (id INT NOT NULL, q `x.y`);
```

```sql
SELECT id, q FROM t
-- inferred: None
```
