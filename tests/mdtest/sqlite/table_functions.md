# SQLite table-valued functions

`json_each` and `json_tree` are row sources with SQLite's columns. `key`,
`value` and `atom` take any type.

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER PRIMARY KEY, note TEXT);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

## json_each

```sql
SELECT j.key, j.value, j.type, j.fullkey, j.id, j.parent FROM events e, json_each(e.note) AS j
-- columns: key: None (None)
-- columns: value: None (None)
-- columns: type: str (TEXT)
-- columns: fullkey: str (TEXT)
-- columns: id: int (BIGINT)
-- columns: parent: int | None (BIGINT)
```

```sql
SELECT * FROM json_tree('{"a": [1, 2]}')
-- columns: key: None (None)
-- columns: value: None (None)
-- columns: type: str (TEXT)
-- columns: atom: None (None)
-- columns: id: int (BIGINT)
-- columns: parent: int | None (BIGINT)
-- columns: fullkey: str (TEXT)
-- columns: path: str (TEXT)
```

## jsonb_each and jsonb_tree

`jsonb_each` and `jsonb_tree` have the same columns.

```sql
SELECT j.key, j.type, j.fullkey, j.parent FROM events e, jsonb_each(e.note) AS j
-- columns: key: None (None)
-- columns: type: str (TEXT)
-- columns: fullkey: str (TEXT)
-- columns: parent: int | None (BIGINT)
```

```sql
SELECT type, path FROM jsonb_tree('{"a": [1, 2]}')
-- columns: type: str (TEXT)
-- columns: path: str (TEXT)
```
