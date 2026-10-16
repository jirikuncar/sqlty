# SQLite functions

Built-in [core](https://www.sqlite.org/lang_corefunc.html),
[date](https://www.sqlite.org/lang_datefunc.html),
[math](https://www.sqlite.org/lang_mathfunc.html) and
[JSON](https://www.sqlite.org/json1.html) functions. SQLite has no date type:
the date functions return text, or `NULL` for a value that is not a date.

`schema.sql`:

```sql
CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    note TEXT,
    happened TEXT NOT NULL,
    ratio REAL NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## Control flow

```sql
SELECT IIF(id > 1, name, note) AS a, IIF(id > 1, 'big', 'small') AS b, IFNULL(note, name) AS c FROM events
-- columns: a: str | None (TEXT)
-- columns: b: typing.Literal['big', 'small'] (None)
-- columns: c: str (TEXT)
```

## Dates and times

```sql
SELECT date(happened) AS a, time(happened) AS b, datetime(happened, '+1 day') AS c, strftime('%Y', happened) AS d, julianday(happened) AS e, unixepoch(happened) AS f FROM events
-- columns: a: str | None (TEXT)
-- columns: b: str | None (TEXT)
-- columns: c: str | None (TEXT)
-- columns: d: str | None (TEXT)
-- columns: e: float | None (DOUBLE)
-- columns: f: int | None (BIGINT)
```

## Strings

```sql
SELECT ltrim(name) AS a, hex(name) AS b, printf('%d', id) AS c, instr(name, 'a') AS d, typeof(note) AS e, concat_ws(',', name, note) AS f FROM events
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: str (TEXT)
-- columns: d: int (BIGINT)
-- columns: e: str (TEXT)
-- columns: f: str (TEXT)
```

## Numbers

`mod` is one of the math functions, which return a real; `%` keeps integers.
`total` is `0.0` over no rows, where `sum` is `NULL`.

```sql
SELECT abs(id) AS a, round(ratio) AS b, round(id) AS c, random() AS d, sqrt(id) AS e, floor(ratio) AS f, total(ratio) AS g, mod(id, 2) AS h, id % 2 AS i FROM events
-- columns: a: int (INT)
-- columns: b: float (DOUBLE)
-- columns: c: float (DOUBLE)
-- columns: d: int (BIGINT)
-- columns: e: float (DOUBLE)
-- columns: f: float (DOUBLE)
-- columns: g: float (DOUBLE)
-- columns: h: float (DOUBLE)
-- columns: i: int (INT)
```

## JSON

The JSON functions return text.

```sql
SELECT json_object('id', id) AS a, json_array(id, name) AS b, json_array_length('[1]') AS c, json_valid(name) AS d FROM events
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: int (BIGINT)
-- columns: d: int (BIGINT)
```

`->` gives the element as JSON text. `->>` gives its SQL value, an integer, a
real or text, so its type is not known.

```sql
SELECT note -> '$.a' AS a, note ->> '$.a' AS b FROM events
-- columns: a: str | None (TEXT)
-- columns: b: typing.Any (None)
```

```sql
SELECT json_group_array(id) AS a FROM events
-- columns: a: str | None (TEXT)
```
