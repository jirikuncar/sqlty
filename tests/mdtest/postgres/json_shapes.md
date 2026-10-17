# JSON shapes (PostgreSQL)

A JSON column stays `Any`, but a query that builds it with `jsonb_build_object()`,
`jsonb_agg()` and friends spells out its structure. sqlty keeps that shape next to the
type, and `json-shape-mismatch` checks it against the row type. `-- json:` shows the shape:
`{key: shape}` is an object (`key?` may be missing), `[shape]` an array, `{[key]: shape}` a
map with arbitrary keys, and `| null` marks a value that can be `null`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TYPE address AS (street TEXT, zip TEXT);
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    bio TEXT,
    score NUMERIC(10, 2) NOT NULL,
    active BOOLEAN NOT NULL,
    created TIMESTAMPTZ NOT NULL,
    tags TEXT[] NOT NULL,
    home address,
    prefs JSONB NOT NULL
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total NUMERIC(10, 2) NOT NULL,
    note TEXT
);
```

## jsonb_build_object

Literal keys give an object; each value is the JSON of its SQL type, and a value that can be
NULL is a nullable member.

```sql
SELECT jsonb_build_object(
    'id', id,
    'name', name,
    'bio', bio,
    'score', score,
    'active', active,
    'created', created
) AS doc FROM users
-- json: doc: {id: integer, name: string, bio: string | null, score: number, active: boolean, created: string}
```

## json_build_object

```sql
SELECT json_build_object('id', id, 'tags', tags) AS doc FROM users
-- json: doc: {id: integer, tags: [string]}
```

## Computed keys make a map

```sql
SELECT jsonb_build_object(name, id) AS by_name FROM users
-- json: by_name: {[key]: integer}
```

## Duplicate keys keep the last

```sql
SELECT jsonb_build_object('a', id, 'a', name) AS doc FROM users
-- json: doc: {a: string}
```

## Nested objects

```sql
SELECT jsonb_build_object('user', jsonb_build_object('id', id, 'bio', bio)) AS doc FROM users
-- json: doc: {user: {id: integer, bio: string | null}}
```

## jsonb_build_array

```sql
SELECT jsonb_build_array(id, name) AS pair, jsonb_build_array(id, id + 1) AS ids, jsonb_build_array() AS empty FROM users
-- json: pair: [any]
-- json: ids: [integer]
-- json: empty: [never]
```

## jsonb_agg

```sql
SELECT jsonb_agg(id) AS ids, jsonb_agg(bio) AS bios, json_agg(name) AS names FROM users
-- json: ids: [integer]
-- json: bios: [string | null]
-- json: names: [string]
```

## jsonb_agg of objects, with ORDER BY and FILTER

```sql
SELECT
    jsonb_agg(jsonb_build_object('id', id, 'name', name) ORDER BY id) AS ordered,
    jsonb_agg(jsonb_build_object('id', id) ORDER BY id) FILTER (WHERE active) AS active
FROM users
-- json: ordered: [{id: integer, name: string}]
-- json: active: [{id: integer}]
```

## Nested aggregates

```sql
SELECT jsonb_build_object(
    'id', u.id,
    'orders', (SELECT coalesce(jsonb_agg(jsonb_build_object('id', o.id, 'note', o.note)), '[]') FROM orders o WHERE o.user_id = u.id)
) AS doc FROM users u
-- json: doc: {id: integer, orders: [{id: integer, note: string | null}]}
```

## jsonb_agg of a whole row

```sql
SELECT jsonb_agg(u) AS everyone FROM users u
-- json: everyone: [{id: integer, name: string, bio: string | null, score: number, active: boolean, created: string, tags: [string], home: {street: string | null, zip: string | null} | null, prefs: any}]
```

## jsonb_agg over an outer join can hold nulls

The unmatched row of a `LEFT JOIN` is null-extended, so each member of its object is
nullable.

```sql
SELECT u.id, jsonb_agg(jsonb_build_object('id', o.id, 'total', o.total)) AS orders
FROM users u LEFT JOIN orders o ON o.user_id = u.id GROUP BY u.id
-- json: orders: [{id: integer | null, total: number | null}]
```

## FILTER drops the rows an outer join null-extends

`FILTER (WHERE o.id IS NOT NULL)` keeps the matched rows only, so their members are not
nullable.

```sql
SELECT u.id, jsonb_agg(jsonb_build_object('id', o.id, 'total', o.total)) FILTER (WHERE o.id IS NOT NULL) AS orders
FROM users u LEFT JOIN orders o ON o.user_id = u.id GROUP BY u.id
-- json: orders: [{id: integer, total: number}]
```

## FILTER with a whole-row or NOT NULL column guard

`o IS NOT NULL`, `o.* IS NOT NULL` and a `NOT NULL` column of `o` all prove that the
outer-joined row is there.

```sql
SELECT
    jsonb_agg(jsonb_build_object('total', o.total)) FILTER (WHERE o IS NOT NULL) AS a,
    jsonb_agg(jsonb_build_object('total', o.total)) FILTER (WHERE o.* IS NOT NULL) AS b,
    jsonb_agg(jsonb_build_object('total', o.total)) FILTER (WHERE o.user_id IS NOT NULL) AS c,
    jsonb_agg(jsonb_build_object('total', o.total)) AS unguarded
FROM users u LEFT JOIN orders o ON o.user_id = u.id
-- json: a: [{total: number}]
-- json: b: [{total: number}]
-- json: c: [{total: number}]
-- json: unguarded: [{total: number | null}]
```

## coalesce(jsonb_agg(...), '[]') keeps the element shape

```sql
SELECT coalesce(jsonb_agg(jsonb_build_object('id', id)), '[]') AS items FROM users
-- json: items: [{id: integer}]
-- columns: items: typing.Any (JSONB)
```

## The empty array literal alone

```sql
SELECT '[]'::jsonb AS items, '{}'::jsonb AS empty, '[1, 2.5, null]'::jsonb AS mixed
-- json: items: [never]
-- json: empty: {}
-- json: mixed: [number | null]
```

## Literals

```sql
SELECT '{"a": 1, "b": [true, "x"], "c": null}'::jsonb AS doc
-- json: doc: {a: integer, b: [any], c: any | null}
```

## Invalid literal has no shape

```sql
SELECT '{'::jsonb AS doc
-- json: None
```

## to_jsonb of scalars, arrays and rows

```sql
SELECT to_jsonb(name) AS name, to_jsonb(tags) AS tags, to_jsonb(home) AS home, to_jsonb(u) AS everything FROM users u
-- json: name: string
-- json: tags: [string]
-- json: home: {street: string | null, zip: string | null}
-- json: everything: {id: integer, name: string, bio: string | null, score: number, active: boolean, created: string, tags: [string], home: {street: string | null, zip: string | null} | null, prefs: any}
```

## row_to_json

```sql
SELECT row_to_json(o) AS doc FROM orders o
-- json: doc: {id: integer, user_id: integer, total: number, note: string | null}
```

## jsonb_object_agg is a map

```sql
SELECT jsonb_object_agg(name, id) AS ids, jsonb_object_agg(name, jsonb_build_object('bio', bio)) AS bios FROM users
-- json: ids: {[key]: integer}
-- json: bios: {[key]: {bio: string | null}}
```

## coalesce(jsonb_object_agg(...), '{}') stays a map

```sql
SELECT coalesce(jsonb_object_agg(name, id), '{}') AS ids FROM users
-- json: ids: {[key]: integer}
```

## Following -> and #>

`->` walks into an object by key or an array by index. A key the shape does not list, or a
shape that is not known, gives no shape.

```sql
SELECT
    jsonb_build_object('a', jsonb_build_object('b', id)) -> 'a' AS a,
    jsonb_build_object('a', jsonb_build_object('b', id)) -> 'a' -> 'b' AS b,
    jsonb_build_object('a', jsonb_build_object('b', id)) #> '{a,b}' AS path,
    jsonb_agg(name) -> 0 AS first,
    jsonb_build_object('a', 1) -> 'missing' AS missing,
    prefs -> 'theme' AS opaque
FROM users
GROUP BY prefs
-- json: a: {b: integer}
-- json: b: integer
-- json: path: integer
-- json: first: string
```

`jsonb_extract_path(doc, 'a', 'b')` and `json_extract_path` follow their keys like `#>`.

```sql
SELECT
    jsonb_extract_path(jsonb_build_object('a', jsonb_build_object('b', id)), 'a', 'b') AS b,
    jsonb_extract_path(jsonb_build_object('a', jsonb_build_object('b', id)), 'a') AS a,
    json_extract_path(json_build_object('a', name), 'a') AS name,
    jsonb_extract_path(jsonb_build_object('a', 1), 'missing') AS missing,
    jsonb_extract_path(prefs, 'theme') AS opaque
FROM users
-- json: b: integer
-- json: a: {b: integer}
-- json: name: string
```

## ->> is text

```sql
SELECT jsonb_build_object('a', id) ->> 'a' AS a FROM users
-- json: None
-- columns: a: str | None (TEXT)
```

## CASE merges objects

An object built by only some branches has optional members.

```sql
SELECT CASE WHEN active THEN jsonb_build_object('id', id, 'a', 1) ELSE jsonb_build_object('id', id, 'b', 'x') END AS doc FROM users
-- json: doc: {id: integer, a?: integer, b?: string}
```

## A shape survives a CTE and a subquery

```sql
WITH docs AS (SELECT id, jsonb_build_object('id', id, 'name', name) AS doc FROM users)
SELECT doc FROM docs
-- json: doc: {id: integer, name: string}
```

```sql
SELECT doc FROM (SELECT jsonb_agg(id) AS doc FROM users) AS t
-- json: doc: [integer]
```

## A set operation unifies the shapes of its branches

```sql
SELECT jsonb_build_object('id', id) AS doc FROM users
UNION ALL
SELECT jsonb_build_object('id', id, 'note', note) FROM orders
-- json: doc: {id: integer, note?: string | null}
```

```sql
SELECT jsonb_build_object('id', id) AS doc FROM users
UNION ALL
SELECT prefs FROM users
-- json: None
```

## Columns of unknown shape have none

```sql
SELECT prefs, prefs || '{}'::jsonb AS merged, jsonb_set(prefs, '{a}', '1') AS updated FROM users
-- json: None
```
