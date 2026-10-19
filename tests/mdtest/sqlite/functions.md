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

`printf` and `format` return NULL instead of an empty string, so `printf('%s', name)` is
NULL for an empty `name`.

```sql
SELECT ltrim(name) AS a, hex(name) AS b, printf('%d', id) AS c, instr(name, 'a') AS d, typeof(note) AS e, concat_ws(',', name, note) AS f FROM events
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
-- columns: c: str | None (TEXT)
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
-- columns: e: float | None (DOUBLE)
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

`json_pretty` (SQLite 3.46) indents JSON text and raises an error on text that is not JSON:

```sql
SELECT json_pretty(name) AS a, json_pretty(note, '  ') AS b FROM events
-- columns: a: str (TEXT)
-- columns: b: str | None (TEXT)
```

`->` gives the element as JSON text. `->>` and `json_extract` give its SQL value,
an integer, a real or text, so its type is not known.

```sql
SELECT note -> '$.a' AS a, note ->> '$.a' AS b, json_extract(note, '$.a') AS c FROM events
-- columns: a: str | None (TEXT)
-- columns: b: typing.Any (None)
-- columns: c: typing.Any (None)
```

`json_group_array` and `json_group_object` are an empty array or object over no rows.

```sql
SELECT json_group_array(id) AS a, json_group_object(name, note) AS b FROM events WHERE id < 0
-- columns: a: str (TEXT)
-- columns: b: str (TEXT)
```

`jsonb` stores JSON as a blob; `json_error_position` is where the text stops being JSON.

```sql
SELECT jsonb(note) AS a, json_error_position(note) AS b FROM events
-- columns: a: bytes | None (BLOB)
-- columns: b: int | None (BIGINT)
```

Each `jsonb_` function works as its `json_` one, but returns JSONB, a blob.
`jsonb_extract` returns a scalar element as its SQL value.

```sql
SELECT jsonb_array(id, note) AS a, jsonb_object('n', note) AS b, jsonb_set(note, '$.a', 1) AS c, jsonb_insert(note, '$.a', 1) AS d, jsonb_replace(note, '$.a', 1) AS e, jsonb_remove(note, '$.a') AS f, jsonb_patch(note, '{}') AS g, jsonb_array_insert(note, '$[0]', 1) AS h, json_array_insert(note, '$[0]', 1) AS i, jsonb_extract(note, '$.a') AS k, jsonb_group_array(id) AS l, jsonb_group_object(name, note) AS m FROM events
-- columns: a: bytes (BLOB)
-- columns: b: bytes (BLOB)
-- columns: c: bytes | None (BLOB)
-- columns: d: bytes | None (BLOB)
-- columns: e: bytes | None (BLOB)
-- columns: f: bytes | None (BLOB)
-- columns: g: bytes | None (BLOB)
-- columns: h: bytes | None (BLOB)
-- columns: i: str | None (TEXT)
-- columns: k: typing.Any (None)
-- columns: l: bytes (BLOB)
-- columns: m: bytes (BLOB)
-- rules: a: sqlite.json.blob-non-null / sqlite.json.blob-non-null
-- rules: b: sqlite.json.blob-non-null / sqlite.json.blob-non-null
-- rules: c: sqlite.json.blob / sqlite.json.blob
-- rules: d: sqlite.json.blob / sqlite.json.blob
-- rules: e: sqlite.json.blob / sqlite.json.blob
-- rules: f: sqlite.json.blob / sqlite.json.blob
-- rules: g: sqlite.json.blob / sqlite.json.blob
-- rules: h: sqlite.json.blob / sqlite.json.blob
-- rules: i: sqlite.json.text / sqlite.json.text
-- rules: k: sqlite.json.jsonb-extract / sqlite.json.jsonb-extract
-- rules: l: sqlite.json.aggregate-blob / sqlite.json.aggregate-blob
-- rules: m: sqlite.json.aggregate-blob / sqlite.json.aggregate-blob
```

## Percentiles

The [percentile extension](https://www.sqlite.org/percentile.html) adds `median`,
`percentile`, `percentile_cont` and `percentile_disc`. Each returns a real, and `NULL`
when every input is `NULL`, even within a group.

```sql
SELECT median(ratio) AS a, percentile(id, 90) AS b, percentile_cont(ratio, 0.5) AS c, percentile_disc(id, 0.5) AS d FROM events GROUP BY name
-- columns: a: float | None (DOUBLE)
-- columns: b: float | None (DOUBLE)
-- columns: c: float | None (DOUBLE)
-- columns: d: float | None (DOUBLE)
-- rules: a: sqlite.aggregate.percentile / sqlite.aggregate.percentile
-- rules: b: sqlite.aggregate.percentile / sqlite.aggregate.percentile
-- rules: c: sqlite.aggregate.percentile / sqlite.aggregate.percentile
-- rules: d: sqlite.aggregate.percentile / sqlite.aggregate.percentile
```

## Window functions and NULLIF

```sql
SELECT lag(id) OVER (ORDER BY id) AS a, lead(name, 1, 'x') OVER (ORDER BY id) AS b, first_value(ratio) OVER (ORDER BY id) AS c, nth_value(name, 2) OVER (ORDER BY id) AS d, percent_rank() OVER (ORDER BY id) AS e, cume_dist() OVER (ORDER BY id) AS f, nullif(name, 'x') AS g FROM events
-- columns: a: int | None (INT)
-- columns: b: str (TEXT)
-- columns: c: float (FLOAT)
-- columns: d: str | None (TEXT)
-- columns: e: float (DOUBLE)
-- columns: f: float (DOUBLE)
-- columns: g: str | None (TEXT)
```

## Blobs and other built-ins

`unhex` is `NULL` for text that is not hexadecimal. `likely` and `likelihood`
pass their argument through, and the `glob` and `like` functions are 0 or 1.

```sql
SELECT zeroblob(4) AS a, randomblob(4) AS b, unhex(name) AS c, sqlite_version() AS d, likely(id > 1) AS e, likelihood(name, 0.5) AS f, glob('a*', name) AS g, like('a%', name) AS h FROM events
-- columns: a: bytes (BLOB)
-- columns: b: bytes (BLOB)
-- columns: c: bytes | None (BLOB)
-- columns: d: str (TEXT)
-- columns: e: int (BOOLEAN)
-- columns: f: str (TEXT)
-- columns: g: int (BIGINT)
-- columns: h: int (BIGINT)
```

`unistr` reads backslash escapes, and `unistr_quote` quotes any value, `NULL`
included. `sqlite_compileoption_get(N)` is `NULL` past the last option.

```sql
SELECT unistr(note) AS a, unistr_quote(note) AS b, sqlite_compileoption_get(id) AS c, sqlite_compileoption_used(name) AS d FROM events
-- columns: a: str | None (TEXT)
-- columns: b: str (TEXT)
-- columns: c: str | None (TEXT)
-- columns: d: int (BIGINT)
```

A blob literal is a blob.

```sql
SELECT X'41' AS a FROM events
-- columns: a: bytes (VARBINARY)
```

## Blob substrings

`substr` of a blob is a blob; `trim` always returns text.

`schema.sql`:

```sql
CREATE TABLE files (id INTEGER PRIMARY KEY, data BLOB NOT NULL);
```

```sql
SELECT substr(data, 1, 2) AS a, trim(data) AS b FROM files
-- columns: a: bytes (VARBINARY)
-- columns: b: str (TEXT)
```

## Bitwise operators

`&`, `|` and `~` convert their operands to 64-bit integers.

```sql
SELECT id & 3 AS a, id | 4 AS b, ~id AS c, name & 1 AS d, note | 1 AS e FROM events
-- columns: a: int (BIGINT)
-- columns: b: int (BIGINT)
-- columns: c: int (BIGINT)
-- columns: d: int (BIGINT)
-- columns: e: int | None (BIGINT)
```

So do the shifts `<<` and `>>`.

```sql
SELECT id << 2 AS a, note >> 1 AS b, id << 1 | 1 AS c FROM events
-- columns: a: int (BIGINT)
-- columns: b: int | None (BIGINT)
-- columns: c: int (BIGINT)
-- rules: a: sqlite.bitwise / sqlite.bitwise
-- rules: b: sqlite.bitwise / sqlite.bitwise
-- rules: c: sqlite.bitwise / sqlite.bitwise
```

## Branches of different types

SQLite gives an expression no type: a `CASE` returns whichever branch's value
runs, so branches of different types give their union. Numbers widen.
`ceil` and `floor` return an integer argument as it is.

```sql
SELECT CASE WHEN id > 1 THEN id ELSE name END AS a, coalesce(note, ratio) AS b, iif(id > 1, id, ratio) AS c, max(name, id) AS d, ceil(id) AS e, floor(ratio) AS f FROM events
-- columns: a: int | str (None)
-- columns: b: str | float (None)
-- columns: c: float (FLOAT)
-- columns: d: str | int (None)
-- columns: e: int (INT)
-- columns: f: float (DOUBLE)
```

```sql
SELECT id AS a FROM events UNION ALL SELECT name FROM events
-- columns: a: int | str (None)
```

## Subqueries without an alias

SQLite reads a subquery in `FROM` without an alias; its columns are referenced
unqualified.

```sql
SELECT n, upper FROM (SELECT id AS n, upper(name) AS upper FROM events)
-- columns: n: int (INT)
-- columns: upper: str (TEXT)
```

## Index hints

`INDEXED BY` and `NOT INDEXED` only pick the index a query uses, so sqlty reads the
query without them:

```sql
SELECT e.id FROM events AS e INDEXED BY events_name JOIN events AS f NOT INDEXED ON e.id = f.id
-- columns: id: int (INT)
```

## IS and IS NOT

SQLite's `a IS b` compares any two values, `NULL` included: it is
`a IS NOT DISTINCT FROM b`, and `a IS NOT b` is `a IS DISTINCT FROM b`. Both are
never `NULL`.

```sql
SELECT note IS name AS a, note IS NOT 'x' AS b, note IS NULL AS c FROM events
-- columns: a: int (BOOLEAN)
-- columns: b: int (BOOLEAN)
-- columns: c: int (BOOLEAN)
```

`x ISNULL` is `x IS NULL`, and `x NOTNULL` is `x IS NOT NULL`.

```sql
SELECT note ISNULL AS a, note NOTNULL AS b FROM events WHERE name ISNULL
-- columns: a: int (BOOLEAN)
-- columns: b: int (BOOLEAN)
```

## Window frames with EXCLUDE

A frame that excludes the current row may be empty, so `sum` over it is `NULL`
for a row with no neighbours.

```sql
SELECT sum(id) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING EXCLUDE CURRENT ROW) AS a FROM events
-- columns: a: int | None (INT)
```
