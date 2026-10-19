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

In the select list, the values have the same types: PostgreSQL has no
`generate_series` over dates, so it casts them to `timestamptz`.

```sql
SELECT generate_series('2024-01-01'::DATE, '2024-01-07'::DATE, '1 day') AS day
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

A text search vector unnests into its lexemes, with their positions and weights:

```sql
SELECT * FROM unnest('cat:3 fat:2,4 rat:5A'::tsvector)
-- columns: lexeme: str (TEXT)
-- columns: positions: list[int] | None (ARRAY<SMALLINT>)
-- columns: weights: list[str] | None (ARRAY<TEXT>)
```

## JSON

```sql
SELECT * FROM jsonb_path_query('{"a": [1, 2]}', '$.a[*]')
-- columns: jsonb_path_query: typing.Any (JSONB)
```

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

### A row type

`jsonb_populate_record` and `jsonb_populate_recordset` return rows of the table or
composite type their first argument is cast to. Each column can be NULL: a key the
JSON lacks takes the field of the base row, NULL in `NULL::posts`.

`schema.sql`:

```sql
CREATE TABLE posts (id INTEGER PRIMARY KEY, tags TEXT[] NOT NULL, doc JSONB NOT NULL, published TIMESTAMPTZ NOT NULL);
CREATE TYPE point2 AS (x INTEGER, label TEXT);
```

```sql
SELECT p.id, p.tags FROM jsonb_populate_recordset(NULL::posts, '[{"id": 1}]') AS p
-- columns: id: int | None (INT)
-- columns: tags: list[str] | None (ARRAY<TEXT>)
```

```sql
SELECT * FROM json_populate_record(NULL::point2, '{"x": 1}') AS r(a, b)
-- columns: a: int | None (INT)
-- columns: b: str | None (TEXT)
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

`string_to_table` gives NULL for the pieces equal to its third argument:

```sql
SELECT * FROM regexp_matches('abc', '(b)(c)', 'g') AS m, string_to_table('a,,b', ',', '') AS piece
-- columns: m: list[str] (ARRAY<TEXT>)
-- columns: piece: str | None (TEXT)
```

## Functions with OUT parameters

A built-in function that returns `record` through OUT parameters, such as
`ts_stat` or `pg_stat_file`, has those as its columns. Each can be NULL:

```sql
SELECT s.word, s.ndoc FROM ts_stat('SELECT to_tsvector(''a b'')') AS s
-- columns: word: str | None (TEXT)
-- columns: ndoc: int | None (INT)
```

```sql
SELECT * FROM pg_stat_file('postmaster.pid')
-- columns: size: int | None (BIGINT)
-- columns: access: datetime.datetime | None (TIMESTAMPTZ)
-- columns: modification: datetime.datetime | None (TIMESTAMPTZ)
-- columns: change: datetime.datetime | None (TIMESTAMPTZ)
-- columns: creation: datetime.datetime | None (TIMESTAMPTZ)
-- columns: isdir: bool | None (BOOLEAN)
```

## Other functions

Any other function in `FROM` is one row source with one column of its result, as
the rules type it in a select list:

```sql
SELECT * FROM generate_subscripts(ARRAY[1, 2], 1) AS i, lower('A'), posts, upper(posts.tags[1]) AS tag
-- columns: i: int (INT)
-- columns: lower: str (TEXT)
-- columns: id: int (INT)
-- columns: tags: list[str] (ARRAY<TEXT>)
-- columns: doc: typing.Any (JSONB)
-- columns: published: datetime.datetime (TIMESTAMPTZ)
-- columns: tag: str | None (TEXT)
```

An aggregate is not allowed in `FROM`, so its star stays unknown:

```sql
SELECT * FROM count(1)
-- columns: *: None (None)
```

## Several in a select list

PostgreSQL runs the set-returning functions of one select list in lockstep and
pads the shorter ones with NULL, so each of them can be NULL. One alone cannot.

```sql
SELECT generate_series(1, 2) AS a, unnest(tags) AS tag, unnest(tags) || '!' AS loud, COALESCE(generate_series(1, 3), 0) AS c, id FROM posts
-- columns: a: int | None (INT)
-- columns: tag: str | None (TEXT)
-- columns: loud: str | None (TEXT)
-- columns: c: int (INT)
-- columns: id: int (INT)
```

```sql
SELECT generate_series(1, id) AS n, id FROM posts
-- columns: n: int (INT)
-- columns: id: int (INT)
```

## JSON_TABLE

PostgreSQL 17's `JSON_TABLE` takes its columns from its `COLUMNS` clause.

```sql
SELECT jt.x, jt.n FROM JSON_TABLE('[1, {}]'::jsonb, '$[*]' COLUMNS (n FOR ORDINALITY, x int PATH '$')) AS jt
-- columns: x: int | None (INT)
-- columns: n: int (INT)
```

## XMLTABLE

`XMLTABLE` takes its columns from its `COLUMNS` clause too. A column is NULL where
its path matches nothing, unless it is `NOT NULL` or has a constant `DEFAULT`:

```sql
SELECT * FROM XMLTABLE('/r/i' PASSING ('<r><i a="1">x</i></r>'::xml) COLUMNS n FOR ORDINALITY, a int PATH '@a', v text PATH '.' NOT NULL, d text PATH 'd' DEFAULT '-') AS x
-- columns: n: int (INT)
-- columns: a: int | None (INT)
-- columns: v: str (TEXT)
-- columns: d: str (TEXT)
```

The options of a column come in any order, and a bare `NULL` is the default:

```sql
SELECT * FROM XMLTABLE('/r' PASSING ('<r><c>1</c></r>'::xml) COLUMNS a int NOT NULL PATH 'c', b text DEFAULT 'x' PATH 'b', c text NULL PATH 'c', d text NOT NULL DEFAULT 'y' PATH 'd') AS x
-- columns: a: int (INT)
-- columns: b: str (TEXT)
-- columns: c: str | None (TEXT)
-- columns: d: str (TEXT)
```

Without an alias, its name is `xmltable`, and a column list renames it:

```sql
SELECT xmltable.a FROM XMLTABLE('/r' PASSING (XMLPARSE(DOCUMENT '<r a="1"/>')) COLUMNS a int PATH '@a')
-- columns: a: int | None (INT)
```

```sql
SELECT * FROM XMLTABLE('/r' PASSING ('<r/>'::xml) COLUMNS a int, b numeric(5, 2)) AS x(p, q)
-- columns: p: int | None (INT)
-- columns: q: decimal.Decimal | None (DECIMAL(5, 2))
```

## Stars

`*` and `t.*` expand to the columns of a function in `FROM`, with or without
`WITH ORDINALITY`, and to the columns of a `JSON_TABLE`.

```sql
SELECT * FROM unnest(ARRAY[1, 2]) WITH ORDINALITY AS u(x, o)
-- columns: x: int | None (INT)
-- columns: o: int (BIGINT)
```

```sql
SELECT p.id, u.* FROM posts p, unnest(p.tags) WITH ORDINALITY u
-- columns: id: int (INT)
-- columns: u: str | None (TEXT)
-- columns: ordinality: int (BIGINT)
```

```sql
SELECT unnest.* FROM unnest(ARRAY['a'])
-- columns: unnest: str | None (TEXT)
```

```sql
SELECT p.id, jt.* FROM posts p, JSON_TABLE(p.doc, '$.items[*]' COLUMNS (n FOR ORDINALITY, name text PATH '$.name')) AS jt
-- columns: id: int (INT)
-- columns: n: int (INT)
-- columns: name: str | None (TEXT)
```

## Input validation

```sql
SELECT * FROM pg_input_error_info('42000000000', 'integer')
-- columns: message: str | None (TEXT)
-- columns: detail: str | None (TEXT)
-- columns: hint: str | None (TEXT)
-- columns: sql_error_code: str | None (TEXT)
```
