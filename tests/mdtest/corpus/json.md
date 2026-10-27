# Corpus: JSON

`->` and `->>` are NULL when the key is missing, even on a NOT NULL column. JSON builders
never return NULL; JSON aggregates do over no rows. JSON values are `typing.Any`, which
already admits `None`, so a `::text` cast shows their nullability.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## ->> with a cast and with COALESCE

A cast keeps the NULL of a missing key, and `COALESCE` with a literal removes it.

`schema.sql`:

```sql
CREATE TABLE jobs (id BIGSERIAL PRIMARY KEY, data JSONB NOT NULL);
```

```sql
SELECT id,
       data ->> 'PhoneNumber' AS phone_number,
       (data ->> 'ContactName')::text AS contact_name,
       COALESCE(data ->> 'State', '') AS state
FROM jobs
-- inferred: tuple[int, str | None, str | None, str]
```

## json_build_object over a LEFT JOIN

`json_build_object` builds an object with null members rather than returning NULL, while a
`CASE` with a NULL branch is nullable.

`schema.sql`:

```sql
CREATE TABLE chatters (id INTEGER PRIMARY KEY, username TEXT NOT NULL);
CREATE TABLE rooms (
    id INTEGER PRIMARY KEY,
    owner_id INTEGER NOT NULL REFERENCES chatters(id),
    receiver_name TEXT
);
```

```sql
SELECT r.id,
       json_build_object('id', o.id, 'username', o.username)::text AS owner,
       json_build_object('id', c.id, 'username', c.username)::text AS receiver,
       (CASE WHEN c.id IS NULL THEN NULL
             ELSE json_build_object('id', c.id, 'username', c.username)
        END)::text AS receiver_or_null
FROM rooms r
JOIN chatters o ON o.id = r.owner_id
LEFT JOIN chatters c ON c.username = r.receiver_name
-- inferred: tuple[int, str, str, str | None]
```

## jsonb_agg with and without COALESCE

`jsonb_agg` over no rows is NULL; `COALESCE` with an empty array is not.

`schema.sql`:

```sql
CREATE TABLE team (id SERIAL PRIMARY KEY, name TEXT NOT NULL);
```

```sql
SELECT jsonb_agg(team)::text AS teams,
       coalesce(jsonb_agg(team), '[]'::jsonb)::text AS teams_or_empty
FROM team
-- inferred: tuple[str | None, str]
```
