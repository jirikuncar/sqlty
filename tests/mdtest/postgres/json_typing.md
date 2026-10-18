# PostgreSQL JSON Typing

```toml
[tool.sqlty]
dialect = "postgres"
```

This file tests JSON/JSONB type inference. Currently JSON/JSONB maps to `dict`.

## JSON column cast

```sql
SELECT metadata::JSON FROM users
-- inferred: tuple[typing.Any]
```

## JSONB column cast

```sql
SELECT metadata::JSONB FROM users
-- inferred: tuple[typing.Any]
```

## JSON field extraction with ->> (returns text)

```sql
SELECT id::INTEGER, (data->>'name')::TEXT FROM users
-- inferred: tuple[int, str | None]
```

## Nested JSON extraction

```sql
SELECT id::INTEGER, (data->'address'->>'city')::TEXT FROM users
-- inferred: tuple[int, str | None]
```

## JSON extraction with cast to non-text type

```sql
SELECT id::INTEGER, (data->>'age')::INTEGER FROM users
-- inferred: tuple[int, int | None]
```

## json_build_object — basic

```sql
SELECT json_build_object(
    'id', id,
    'name', name
)::JSON FROM users
-- inferred: tuple[typing.Any]
```

## json_build_object — with typed fields

```sql
SELECT json_build_object(
    'id', id::INTEGER,
    'name', name::TEXT,
    'email', email::TEXT
)::JSON FROM users
-- inferred: tuple[typing.Any]
```

## jsonb_build_object — basic

```sql
SELECT jsonb_build_object(
    'user_id', u.id,
    'username', u.name,
    'order_count', COUNT(o.id)
)::JSONB
FROM users u
LEFT JOIN orders o ON u.id = o.user_id
GROUP BY u.id, u.name
-- inferred: tuple[typing.Any]
```

## JSON with mixed non-JSON columns

```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    u.created_at::TIMESTAMP,
    u.external_id::UUID,
    json_build_object(
        'preferences', u.preferences,
        'last_ip', u.last_ip
    )::JSON,
    u.active::BOOLEAN
FROM users u
-- inferred: tuple[int, str, datetime.datetime, uuid.UUID, typing.Any, bool]
```

## SQL/JSON functions

PostgreSQL 16 and 17 added the SQL standard JSON functions. A `RETURNING`
clause gives the result type; without it, `JSON_VALUE` returns `text` and
`JSON_QUERY` returns `jsonb`. `JSON_VALUE` and `JSON_QUERY` are NULL when the
path matches nothing.

`schema.sql`:

```sql
CREATE TABLE docs (id INTEGER NOT NULL, body JSONB NOT NULL, raw TEXT);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT JSON_VALUE(body, '$.name') AS name, JSON_VALUE(body, '$.age' RETURNING INTEGER) AS age, JSON_QUERY(body, '$.tags') AS tags, JSON_QUERY(body, '$.tags' RETURNING TEXT) AS tags_text, JSON_EXISTS(body, '$.name') AS has_name FROM docs
-- columns: name: str | None (TEXT)
-- columns: age: int | None (INT)
-- columns: tags: typing.Any (JSONB)
-- columns: tags_text: str | None (TEXT)
-- columns: has_name: bool (BOOLEAN)
```

```sql
SELECT raw IS JSON AS valid, id::TEXT IS NOT JSON OBJECT AS not_object, JSON_SCALAR(id) AS scalar, JSON(raw) AS parsed, JSON_SERIALIZE(body::JSON RETURNING BYTEA) AS bytes FROM docs
-- columns: valid: bool | None (BOOLEAN)
-- columns: not_object: bool (BOOLEAN)
-- columns: scalar: typing.Any (JSON)
-- columns: parsed: typing.Any (JSON)
-- columns: bytes: bytes (VARBINARY)
```

```sql
SELECT JSON_ARRAY(id, raw) AS arr, JSON_ARRAY(id RETURNING JSONB) AS arr_b, JSON_ARRAYAGG(id) AS ids, JSON_OBJECTAGG(raw: id) AS by_raw FROM docs GROUP BY id, raw
-- columns: arr: typing.Any (JSON)
-- columns: arr_b: typing.Any (JSONB)
-- columns: ids: typing.Any (JSON)
-- columns: by_raw: typing.Any (JSON)
```
