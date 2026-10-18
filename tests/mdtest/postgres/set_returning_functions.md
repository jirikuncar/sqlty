# Set-returning functions in FROM

The built-in set-returning functions are row sources with known columns, named
and typed as PostgreSQL does. A function with one unnamed result column takes
its name from the function, or from a bare table alias; a column list renames
the columns in order, and `WITH ORDINALITY` adds a `bigint` column
`ordinality`. Function calls in `FROM` may read the relations before them.

`schema.sql`:

```sql
CREATE TABLE posts (id INTEGER PRIMARY KEY, tags TEXT[] NOT NULL, doc JSONB NOT NULL, published TIMESTAMPTZ NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## generate_series

The result follows the type of the bounds.

```sql
SELECT * FROM generate_series(1, 3)
-- columns: generate_series: int (INT)
```

```sql
SELECT * FROM generate_series(1, 3) AS n
-- columns: n: int (INT)
```

```sql
SELECT * FROM generate_series(1::BIGINT, 3) WITH ORDINALITY AS g(n, i)
-- columns: n: int (BIGINT)
-- columns: i: int (BIGINT)
```

```sql
SELECT * FROM generate_series(1.5, 3)
-- columns: generate_series: decimal.Decimal (DECIMAL)
```

```sql
SELECT * FROM generate_series('2024-01-01'::TIMESTAMP, '2024-01-02'::TIMESTAMP, INTERVAL '1 hour')
-- columns: generate_series: datetime.datetime (TIMESTAMP)
```

```sql
SELECT * FROM generate_series('2024-01-01'::DATE, '2024-01-07'::DATE, INTERVAL '1 day') AS day
-- columns: day: datetime.datetime (TIMESTAMPTZ)
```

## unnest

Each array gives a column of its element type, which can be NULL.

```sql
SELECT * FROM unnest(ARRAY['a', 'b'])
-- columns: unnest: str | None (TEXT)
```

```sql
SELECT * FROM unnest(ARRAY[1, 2], ARRAY['a', 'b']) AS u(n, s)
-- columns: n: int | None (INT)
-- columns: s: str | None (TEXT)
```

```sql
SELECT p.id, t.tag FROM posts p, unnest(p.tags) AS t(tag)
-- columns: id: int (INT)
-- columns: tag: str | None (TEXT)
```

```sql
SELECT p.id, t FROM posts p CROSS JOIN LATERAL unnest(p.tags) WITH ORDINALITY AS t
-- columns: id: int (INT)
-- columns: t: str | None (TEXT)
```

## JSON

```sql
SELECT * FROM jsonb_array_elements('[1, 2]')
-- columns: value: typing.Any (JSONB)
```

```sql
SELECT * FROM jsonb_array_elements_text('[1, 2]') AS e
-- columns: value: str | None (TEXT)
```

```sql
SELECT p.id, j.key, j.value FROM posts p, jsonb_each(p.doc) AS j
-- columns: id: int (INT)
-- columns: key: str (TEXT)
-- columns: value: typing.Any (JSONB)
```

```sql
SELECT * FROM json_each_text('{"a": 1}') AS j(k, v)
-- columns: k: str (TEXT)
-- columns: v: str | None (TEXT)
```

```sql
SELECT * FROM jsonb_object_keys('{"a": 1}')
-- columns: jsonb_object_keys: str (TEXT)
```

### A column definition list

A function that returns `record`, such as `jsonb_to_recordset`, takes its columns
from the definition list after its alias. Each can be NULL: a record that lacks the
member has a NULL there.

```sql
SELECT p.id, r.name, r.qty FROM posts p, jsonb_to_recordset(p.doc) AS r(name text, qty int)
-- columns: id: int (INT)
-- columns: name: str | None (TEXT)
-- columns: qty: int | None (INT)
```

```sql
SELECT * FROM json_to_record('{"a": 1}') AS t(a numeric, b date)
-- columns: a: decimal.Decimal | None (DECIMAL)
-- columns: b: datetime.date | None (DATE)
```

## Text

```sql
SELECT * FROM regexp_split_to_table('a b c', ' ') AS word
-- columns: word: str (TEXT)
```

## Several in a select list

PostgreSQL runs the set-returning functions of one select list in lockstep and
pads the shorter ones with NULL, so each of them can be NULL. One alone cannot.

```sql
SELECT generate_series(1, 2) AS a, unnest(tags) AS tag, unnest(tags) || '!' AS loud, COALESCE(generate_series(1, 3), 0) AS c, id FROM posts
-- columns: a: int | None (None)
-- columns: tag: str | None (TEXT)
-- columns: loud: str | None (TEXT)
-- columns: c: int (None)
-- columns: id: int (INT)
```

```sql
SELECT generate_series(1, id) AS n, id FROM posts
-- columns: n: int (None)
-- columns: id: int (INT)
```

## JSON_TABLE

PostgreSQL 17's `JSON_TABLE` takes its columns from its `COLUMNS` clause.

```sql
SELECT jt.x, jt.n FROM JSON_TABLE('[1, {}]'::jsonb, '$[*]' COLUMNS (n FOR ORDINALITY, x int PATH '$')) AS jt
-- columns: x: int | None (INT)
-- columns: n: int (INT)
```
