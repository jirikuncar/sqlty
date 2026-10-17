# MySQL type spellings

Spellings SQLAlchemy 2.0's MySQL dialect writes, read as MySQL reads them.

`schema.sql`:

```sql
CREATE TABLE t (
    n NATIONAL CHAR(10) NOT NULL,
    nv NATIONAL VARCHAR(20),
    ncv NATIONAL CHARACTER VARYING(5),
    r REAL NOT NULL,
    b BLOB NOT NULL,
    s SET('Draft', 'Live') NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## National character types

`NATIONAL CHAR` is `NCHAR`, `NATIONAL VARCHAR` is `NVARCHAR`.

```sql
SELECT n, nv, ncv FROM t
-- columns: n: str (CHAR(10))
-- columns: nv: str | None (VARCHAR(20))
-- columns: ncv: str | None (VARCHAR(5))
```

## REAL, BLOB and SET

MySQL's `REAL` is a `DOUBLE` unless the `REAL_AS_FLOAT` SQL mode is on. A `BLOB`
keeps its name, and `SET` labels keep their case.

```sql
SELECT r, b, s FROM t
-- columns: r: float (DOUBLE)
-- columns: b: bytes (BLOB)
-- columns: s: str (SET('Draft', 'Live'))
```

## CURRENT_USER

MySQL reads `CURRENT_USER` without parentheses as `CURRENT_USER()`.

```sql
SELECT CURRENT_USER, CURRENT_USER() AS u
-- columns: current_user: str (TEXT)
-- columns: u: str (TEXT)
```
