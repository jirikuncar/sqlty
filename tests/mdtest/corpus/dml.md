# Corpus: data-modifying statements

`RETURNING` yields the rows as stored, so its columns keep the table's nullability. A
parameter in `COALESCE($n, col)` takes the column's type and admits NULL, since the column
is the fallback.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## UPDATE with COALESCE parameters

<!-- mdtest:xfail a parameter inside COALESCE is not marked NULL -->

`COALESCE($2, name)` falls back to the stored NOT NULL name, so `$2` may be NULL and `name`
stays non-null.

`schema.sql`:

```sql
CREATE TABLE authors (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, bio TEXT);
```

```sql
UPDATE authors SET name = COALESCE($2, name) WHERE id = $1 RETURNING id, name, bio
-- inferred: tuple[int, str, str | None]
-- params: 1: int (BIGSERIAL)
-- params: 2: str (TEXT) NULL
```

## UPDATE ... RETURNING in a CTE

<!-- mdtest:xfail a parameter inside COALESCE is not marked NULL -->

The CTE exposes the RETURNING columns, and `$4` is typed by `CURRENT_TIMESTAMP`, a
`timestamptz`.

`schema.sql`:

```sql
CREATE TABLE collections (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

```sql
WITH updated AS (
    UPDATE collections SET
        name = COALESCE($2, name),
        description = COALESCE($3, description),
        updated_at = COALESCE($4, CURRENT_TIMESTAMP)
    WHERE id = $1
    RETURNING id, name, description, updated_at
)
SELECT id, name, description, updated_at FROM updated
-- inferred: tuple[int, str, str | None, datetime.datetime]
-- params: 1: int (BIGSERIAL)
-- params: 2: str (TEXT) NULL
-- params: 3: str (TEXT) NULL
-- params: 4: datetime.datetime (TIMESTAMPTZ) NULL
```

## INSERT ... RETURNING * in a CTE, selected through an alias

The inserted rows have the table's columns; `deleted_at` is not written and stays nullable.

`schema.sql`:

```sql
CREATE TABLE customers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    team_id UUID NOT NULL,
    deleted_at TIMESTAMPTZ
);
CREATE TABLE customer_managers (
    customer_id UUID NOT NULL REFERENCES customers(id),
    user_id UUID NOT NULL
);
```

```sql
WITH inserted AS (
    INSERT INTO customers (name, team_id) VALUES ($1, $2)
    RETURNING *
), managers AS (
    INSERT INTO customer_managers (customer_id, user_id)
    SELECT inserted.id, unnest($3::uuid[]) FROM inserted
)
SELECT c.id, c.name, c.team_id, c.deleted_at
FROM inserted AS c
-- inferred: tuple[uuid.UUID, str, uuid.UUID, datetime.datetime | None]
-- params: 1: str (TEXT) NOT NULL
-- params: 2: uuid.UUID (UUID) NOT NULL
-- params: 3: list[uuid.UUID] (ARRAY<UUID>)
```
