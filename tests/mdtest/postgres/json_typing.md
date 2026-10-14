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
-- inferred: tuple[int, str]
```

## Nested JSON extraction

```sql
SELECT id::INTEGER, (data->'address'->>'city')::TEXT FROM users
-- inferred: tuple[int, str]
```

## JSON extraction with cast to non-text type

```sql
SELECT id::INTEGER, (data->>'age')::INTEGER FROM users
-- inferred: tuple[int, int]
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
