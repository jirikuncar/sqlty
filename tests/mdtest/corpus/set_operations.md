# Corpus: set operations

A `UNION` column is nullable when any branch can produce NULL there, and it takes its name
from the first branch.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## UNION ALL of comma joins with a derived table

Every branch has the same nullability per column, so the union keeps it.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, type TEXT NOT NULL);
CREATE TABLE users2 (id INTEGER PRIMARY KEY, type TEXT NOT NULL);
CREATE TABLE old_clients (created_at TIMESTAMP NOT NULL, deleted_at TIMESTAMP);
```

```sql
SELECT u.id, u.type, oc.created_at, oc.deleted_at
FROM users u, (SELECT created_at, deleted_at FROM old_clients) AS oc
UNION ALL
SELECT u.id, u.type, oc.created_at, oc.deleted_at
FROM users2 u, (SELECT created_at, deleted_at FROM old_clients) AS oc
-- inferred: tuple[int, str, datetime.datetime, datetime.datetime | None]
```

## Aggregate over a UNION of CTEs

Every group has a row and `flag` is NOT NULL in both branches, so `bool_and` is never NULL.

`schema.sql`:

```sql
CREATE TABLE things (thing TEXT NOT NULL, flag BOOLEAN NOT NULL);
CREATE TABLE other_things (thing TEXT NOT NULL, flag BOOLEAN NOT NULL);
```

```sql
WITH t AS (
    SELECT thing, flag FROM things WHERE things.thing = $1
), ot AS (
    SELECT thing, flag FROM other_things WHERE other_things.thing = $1
)
SELECT thing, bool_and(flag) AS all_true
FROM (
    SELECT thing, flag FROM t
    UNION
    SELECT thing, flag FROM ot
) u
GROUP BY thing
-- inferred: tuple[str, bool]
-- params: 1: str (TEXT)
```
