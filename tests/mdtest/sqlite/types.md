# SQLite type names

SQLite accepts any type name and derives the column's type affinity from it
([rules](https://www.sqlite.org/datatype3.html#determination_of_column_affinity)).
A name without a mapping of its own is typed by its affinity.

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## Multi-word names

`schema.sql`:

```sql
CREATE TABLE t (
    a UNSIGNED BIG INT NOT NULL,
    b VARYING CHARACTER(255),
    c NATIVE CHARACTER(70) NOT NULL DEFAULT 'x',
    d BIG INT PRIMARY KEY,
    CONSTRAINT one UNIQUE (a)
);
```

```sql
SELECT * FROM t
-- columns: a: int (UNSIGNED_BIG_INT)
-- columns: b: str | None (VARYING_CHARACTER(255))
-- columns: c: str (NATIVE_CHARACTER(70))
-- columns: d: int | None (BIG_INT)
```

## Affinity of unknown names

`schema.sql`:

```sql
CREATE TABLE t (a MYINT, b CHARS, c BLOBBY, d REALLY, e FLOATY, f DOUBLED, g SOMETHING);
```

```sql
SELECT * FROM t
-- columns: a: int | None (MYINT)
-- columns: b: str | None (CHARS)
-- columns: c: bytes | None (BLOBBY)
-- columns: d: float | None (REALLY)
-- columns: e: float | None (FLOATY)
-- columns: f: float | None (DOUBLED)
-- columns: g: float | None (SOMETHING)
```

A cast to such a name follows the same rules.

```sql
SELECT CAST('1' AS MYINT) AS a, CAST(1 AS CHARS) AS b
-- columns: a: int (MYINT)
-- columns: b: str (CHARS)
```

## Columns without a type

A column declared without a type, or as `ANY`, holds any value. It is a column
of the table, with no Python type.

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER PRIMARY KEY, payload, extra ANY);
```

```sql
SELECT * FROM t
-- columns: id: int (INT)
-- columns: payload: None ()
-- columns: extra: None (ANY)
```

`unknown-type` says why they are untyped.

```toml
[tool.sqlty.rules]
unknown-type = "error"
```

```python
from sqlty import SQL

# error: [unknown-type] "column `t.extra` has type `ANY`, which has no Python type"
# error: [unknown-type] "column `t.payload` has no declared type, so it has no Python type"
q = SQL[tuple[int, object, object]]("--sql\nSELECT id, payload, extra FROM t")
```
