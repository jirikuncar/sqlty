# hstore

The `hstore` extension, from `rules/postgres/extensions/hstore.inference.toml`. An
`hstore` value is `dict[str, str | None]`, as psycopg's `register_hstore` loads it.
Its functions are typed once the schema runs `CREATE EXTENSION hstore`; its operators
read an `hstore` operand, so they need no more.

`schema.sql`:

```sql
CREATE EXTENSION IF NOT EXISTS hstore;
CREATE TABLE items (id int PRIMARY KEY, tags hstore NOT NULL, extra hstore, name text NOT NULL);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Columns and casts

```sql
SELECT tags, extra, 'a=>1'::hstore AS cast FROM items
-- columns: tags: dict[str, str | None] (HSTORE)
-- columns: extra: dict[str, str | None] | None (HSTORE)
-- columns: cast: dict[str, str | None] (HSTORE)
```

## Operators

`->` is NULL for a missing key; the other operators are NULL only for a NULL operand.

```sql
SELECT tags -> 'a' AS value, tags -> ARRAY['a', 'b'] AS values, tags || extra AS merged, tags - 'a'::text AS without, tags ? 'a' AS has, tags ?& ARRAY['a'] AS has_all, tags ?| ARRAY['a'] AS has_any, tags @> 'a=>1' AS contains, tags <@ extra AS within FROM items
-- columns: value: str | None (TEXT)
-- columns: values: list[str] (ARRAY<TEXT>)
-- columns: merged: dict[str, str | None] | None (HSTORE)
-- columns: without: dict[str, str | None] (HSTORE)
-- columns: has: bool (BOOLEAN)
-- columns: has_all: bool (BOOLEAN)
-- columns: has_any: bool (BOOLEAN)
-- columns: contains: bool (BOOLEAN)
-- columns: within: bool | None (BOOLEAN)
-- rules: value: postgres.hstore.arrow / postgres.hstore.arrow
-- rules: values: postgres.hstore.arrow-keys / postgres.hstore.arrow-keys
-- rules: merged: postgres.hstore.concat / postgres.hstore.concat
-- rules: without: postgres.hstore.delete / postgres.hstore.delete
-- rules: has: postgres.hstore.has-key / postgres.hstore.has-key
-- rules: has_all: postgres.hstore.has-keys / postgres.hstore.has-keys
-- rules: has_any: postgres.hstore.has-keys / postgres.hstore.has-keys
-- rules: contains: postgres.hstore.contains / postgres.hstore.contains
-- rules: within: postgres.hstore.contains / postgres.hstore.contains
```

## Functions

```sql
SELECT akeys(tags) AS keys, hstore_to_matrix(tags) AS pairs, skeys(tags) AS key, svals(tags) AS value, exist(tags, 'a') AS has, delete(tags, 'a') AS without, hstore_to_json(tags) AS json, hstore_to_jsonb(tags) AS jsonb FROM items
-- columns: keys: list[str] (ARRAY<TEXT>)
-- columns: pairs: list[list[str]] (ARRAY<ARRAY<TEXT>>)
-- columns: key: str (TEXT)
-- columns: value: str | None (TEXT)
-- columns: has: bool (BOOLEAN)
-- columns: without: dict[str, str | None] (HSTORE)
-- columns: json: typing.Any (JSON)
-- columns: jsonb: typing.Any (JSONB)
-- rules: keys: postgres.hstore.arrays / postgres.hstore.arrays
-- rules: pairs: postgres.hstore.matrix / postgres.hstore.matrix
-- rules: key: postgres.hstore.keys / postgres.hstore.keys
-- rules: value: postgres.hstore.values / postgres.hstore.values
-- rules: has: postgres.hstore.tests / postgres.hstore.tests
-- rules: without: postgres.hstore.transform / postgres.hstore.transform
-- rules: json: postgres.hstore.json / postgres.hstore.json
-- rules: jsonb: postgres.hstore.jsonb / postgres.hstore.jsonb
```

A strict function is NULL only for a NULL argument; `hstore(key, value)` only for a NULL key.

```sql
SELECT akeys(extra) AS keys, hstore('a', NULL) AS pair, hstore(NULL, name) AS no_key FROM items
-- columns: keys: list[str] | None (ARRAY<TEXT>)
-- columns: pair: dict[str, str | None] (HSTORE)
-- columns: no_key: dict[str, str | None] | None (HSTORE)
-- rules: keys: postgres.hstore.arrays / postgres.hstore.arrays
-- rules: pair: postgres.hstore.construct / postgres.hstore.construct
-- rules: no_key: postgres.hstore.construct / postgres.hstore.construct
```

A set-returning function reads as a row source:

```sql
SELECT k FROM items, skeys(tags) AS k
-- columns: k: str (TEXT)
```

## Parameters

```sql
SELECT id FROM items WHERE tags -> $1 = $2 AND tags ? $3 AND tags ?| $4 AND tags @> $5
-- params: 1: str (TEXT)
-- params: 2: str (TEXT)
-- params: 3: str (TEXT)
-- params: 4: list[str] (ARRAY<TEXT>)
-- params: 5: dict[str, str | None] (HSTORE)
```

## `types` replaces the Python type

For a project that does not register psycopg's hstore adapter, which then loads text:

```toml
[tool.sqlty.types]
HSTORE = "str"
```

```sql
SELECT tags FROM items
-- columns: tags: str (HSTORE)
```

## Without `CREATE EXTENSION`

The functions are unknown; an `hstore` column is still typed.

`schema.sql`:

```sql
CREATE TABLE items (tags hstore NOT NULL);
```

```sql
SELECT tags, akeys(tags) AS keys FROM items
-- columns: tags: dict[str, str | None] (HSTORE)
-- columns: keys: None (None)
```

```python
from sqlty import SQL

# error: [unknown-function] "function `akeys` is not known to sqlty, so its result is untyped; it comes with `CREATE EXTENSION hstore`, which the schema lacks"
q = SQL[tuple[list[str]]]("--sql\nSELECT akeys(tags) FROM items")
```

## Schemas and `DROP EXTENSION`

An extension created in another schema, or with a version, is the same extension.
`DROP EXTENSION` and `DROP SCHEMA ... CASCADE` remove it.

`schema.sql`:

```sql
CREATE SCHEMA ext;
CREATE EXTENSION hstore WITH SCHEMA ext VERSION '1.8';
CREATE TABLE items (tags ext.hstore NOT NULL);
```

```sql
SELECT ext.akeys(tags) AS keys FROM items
-- columns: keys: list[str] (ARRAY<TEXT>)
```

The schema qualifies only the extension's functions:

```sql
SELECT ext.lower('A') AS lowered FROM items
-- columns: lowered: None (None)
```

### `DROP EXTENSION`

`schema.sql`:

```sql
CREATE EXTENSION hstore;
CREATE TABLE items (tags text NOT NULL);
DROP EXTENSION hstore;
```

```sql
SELECT akeys(tags::hstore) AS keys FROM items
-- columns: keys: None (None)
```

### `DROP SCHEMA ... CASCADE`

`schema.sql`:

```sql
CREATE SCHEMA ext;
CREATE EXTENSION hstore SCHEMA ext;
CREATE TABLE items (tags text NOT NULL);
DROP SCHEMA ext CASCADE;
```

```sql
SELECT akeys(tags::hstore) AS keys FROM items
-- columns: keys: None (None)
```
