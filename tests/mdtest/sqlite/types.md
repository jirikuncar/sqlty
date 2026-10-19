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

## Arithmetic converts operands to numbers

SQLite computes with a string or blob converted to an integer or a real, typed `float`
since either fits it, and with a `BOOLEAN` as the integer it is stored as. Unary `+`
changes nothing, and arithmetic on dates stays untyped (see
[date arithmetic](date_arithmetic.md)).

`schema.sql`:

```sql
CREATE TABLE t (i INTEGER NOT NULL, tx TEXT NOT NULL, b BLOB, d DATE NOT NULL, bo BOOLEAN NOT NULL);
```

```sql
SELECT tx + 1 AS a, i * b AS b, d - 1 AS c, bo + 1 AS d, i / tx AS e, -tx AS f, +tx AS g, abs(b) AS h, sum(tx) AS i FROM t
-- columns: a: float (DOUBLE)
-- columns: b: float | None (DOUBLE)
-- columns: c: None (None)
-- columns: d: int (BIGINT)
-- columns: e: float | None (DOUBLE)
-- columns: f: float (DOUBLE)
-- columns: g: str (TEXT)
-- columns: h: float | None (DOUBLE)
-- columns: i: float | None (DOUBLE)
```

## VALUES lists

A `VALUES` list has the columns `column1`, `column2`, ..., each typed by the types of its
rows and nullable when a row's value is.

`schema.sql`:

```sql
CREATE TABLE t (id INTEGER NOT NULL, name TEXT);
```

```sql
SELECT * FROM (VALUES (1, 'a'), (2, NULL))
-- columns: column1: typing.Literal[1, 2] (None)
-- columns: column2: typing.Literal['a'] | None (None)
```

```sql
SELECT column1 FROM (VALUES (1), (2.5)) AS v
-- columns: column1: float (DOUBLE)
```

```sql
SELECT column1 AS a FROM (VALUES ((SELECT name FROM t LIMIT 1)))
-- columns: a: str | None (TEXT)
```

## CREATE TABLE ... AS SELECT

A table created from a query has the query's column names and types. It has no
constraints, so every column can be NULL.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL CHECK (name IN ('a', 'b')), age INTEGER);
CREATE TABLE users_backup AS SELECT id, name, age, 'v1' AS version FROM users;
```

```sql
SELECT * FROM users_backup
-- columns: id: int | None (INT)
-- columns: name: str | None (TEXT)
-- columns: age: int | None (INT)
-- columns: version: str | None (TEXT)
```

## Virtual tables

The `fts3`, `fts4`, `fts5`, `rtree` and `rtree_i32` modules declare their
columns in `CREATE VIRTUAL TABLE`. An FTS column takes any value, so it has no
type, like a column without one. The hidden `rank` of `fts5` is a REAL, the
`docid` of `fts3` and `fts4` is the rowid; neither is part of `*`. An `rtree`
table has an integer id, then coordinates that are never NULL (REAL, or INTEGER
for `rtree_i32`), and `+` auxiliary columns without a type.

`schema.sql`:

```sql
CREATE VIRTUAL TABLE docs USING fts5(title, body UNINDEXED, tokenize='porter unicode61');
CREATE VIRTUAL TABLE IF NOT EXISTS notes USING fts4(text, tokenize=porter);
CREATE VIRTUAL TABLE boxes USING rtree(id, min_x, max_x, +label);
CREATE VIRTUAL TABLE cells USING rtree_i32(id, x0, x1);
```

```sql
SELECT * FROM docs
-- columns: title: None ()
-- columns: body: None ()
```

```sql
SELECT rowid, rank FROM docs WHERE docs MATCH 'sqlite' ORDER BY rank
-- columns: rowid: int (INT)
-- columns: rank: float | None (FLOAT)
```

```sql
SELECT docid, * FROM notes
-- columns: docid: int (INT)
-- columns: text: None ()
```

```sql
SELECT * FROM boxes
-- columns: id: int (INT)
-- columns: min_x: float (FLOAT)
-- columns: max_x: float (FLOAT)
-- columns: label: None ()
```

```sql
SELECT * FROM cells
-- columns: id: int (INT)
-- columns: x0: int (INT)
-- columns: x1: int (INT)
```
