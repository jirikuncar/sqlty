# Corpus: outer joins

An outer join null-extends the relation on its optional side, whatever that relation's own
constraints are, and only a condition NULL cannot satisfy brings it back. Inner joins keep
each column's declared nullability.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## LEFT JOIN from a derived table

The left side being a derived table changes nothing: an unmatched row still has NULL in every
column of `steps`.

`schema.sql`:

```sql
CREATE TABLE workflows (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE steps (
    id BIGSERIAL PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id),
    position INT NOT NULL,
    title TEXT NOT NULL
);
```

```sql
SELECT wf.id, wf.name, s.id AS step_id, s.position, s.title
FROM (SELECT * FROM workflows w WHERE w.id = $1 LIMIT 1) wf
LEFT JOIN steps s ON s.workflow_id = wf.id
-- inferred: tuple[int, str, int | None, int | None, str | None]
```

## Self LEFT JOIN with COALESCE

The aliased copy is the optional side, and `COALESCE` with a non-null fallback is never NULL.

`schema.sql`:

```sql
CREATE TABLE employees (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    manager_id BIGINT REFERENCES employees(id)
);
```

```sql
SELECT e.id, m.name AS manager_name, COALESCE(m.name, '') AS manager_name_or_empty
FROM employees e
LEFT JOIN employees m ON m.id = e.manager_id
-- inferred: tuple[int, str | None, str]
```

## LEFT JOIN to a filtered derived table

`WHERE title IS NOT NULL` holds inside the derived table, but the outer join still adds rows
where all of its columns are NULL.

`schema.sql`:

```sql
CREATE TABLE users (id UUID PRIMARY KEY, name TEXT);
CREATE TABLE posts (id UUID PRIMARY KEY, user_id UUID NOT NULL REFERENCES users(id), title TEXT);
```

```sql
SELECT u.id, u.name, p.id AS post_id, p.user_id, p.title
FROM users u
LEFT JOIN (SELECT * FROM posts WHERE posts.title IS NOT NULL) p ON p.user_id = u.id
-- inferred: tuple[uuid.UUID, str | None, uuid.UUID | None, uuid.UUID | None, str | None]
```

## Enum column of a LEFT JOINed derived table

An enum column is null-extended like any other.

`schema.sql`:

```sql
CREATE TYPE channel AS ENUM ('online', 'offline');
CREATE TABLE books (id UUID PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE purchases (
    id UUID PRIMARY KEY,
    book_id UUID NOT NULL REFERENCES books(id),
    channel channel NOT NULL,
    created_at TIMESTAMPTZ
);
```

```sql
SELECT b.id, b.name, lp.channel, lp.created_at
FROM books b
LEFT JOIN (
    SELECT pe.book_id, pe.channel, pe.created_at
    FROM purchases pe
    WHERE pe.created_at = (SELECT max(p2.created_at) FROM purchases p2 WHERE p2.book_id = pe.book_id)
) lp ON lp.book_id = b.id
-- inferred: tuple[uuid.UUID, str, typing.Literal['online', 'offline'] | None, datetime.datetime | None]
```

## GROUP BY columns of the optional side

Unmatched rows form a group whose key is NULL, so grouping keeps the columns nullable.

`schema.sql`:

```sql
CREATE TABLE directory_entries (uuid UUID PRIMARY KEY, name TEXT, logo TEXT NOT NULL, archived_at TIMESTAMPTZ);
CREATE TABLE applications (id BIGSERIAL PRIMARY KEY, directory_id UUID);
```

```sql
SELECT ad.name AS directory_name, ad.logo AS directory_logo, ad.uuid AS directory_uuid
FROM applications a
LEFT JOIN (SELECT name, logo, uuid FROM directory_entries WHERE archived_at IS NULL) ad ON ad.uuid = a.directory_id
GROUP BY ad.name, ad.logo, ad.uuid
-- inferred: tuple[str | None, str | None, uuid.UUID | None]
```

## GROUP BY the key of the optional side

The group of unmatched rows has a NULL key and NULL dependent columns, while `count(*)` is
never NULL.

`schema.sql`:

```sql
CREATE TABLE providers (id BIGINT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE sources (id BIGINT PRIMARY KEY, provider_id BIGINT REFERENCES providers(id));
```

```sql
SELECT count(*) AS n, providers.id, providers.name
FROM sources
LEFT JOIN providers ON providers.id = sources.provider_id
GROUP BY providers.id
-- inferred: tuple[int, int | None, str | None]
```

## Inner and left joins in either order

A later inner join that does not reference the left-joined table leaves it optional.

`schema.sql`:

```sql
CREATE TABLE users (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, user_id BIGINT NOT NULL REFERENCES users(id));
CREATE TABLE profiles (id BIGSERIAL PRIMARY KEY, bio TEXT NOT NULL, user_id BIGINT NOT NULL REFERENCES users(id));
```

```sql
SELECT u.name, p.title, pr.bio
FROM posts p
INNER JOIN users u ON p.user_id = u.id
LEFT JOIN profiles pr ON pr.user_id = u.id
-- inferred: tuple[str, str, str | None]
```

```sql
SELECT u.name, p.title, pr.bio
FROM posts p
LEFT JOIN profiles pr ON pr.user_id = p.user_id
INNER JOIN users u ON u.id = p.user_id
-- inferred: tuple[str, str, str | None]
```

## Inner join on a left-joined column through a view

The inner join's condition is false for a NULL `member_team.member_id`, so it drops the
null-extended rows and `member_team` is back to its schema.

`schema.sql`:

```sql
CREATE TABLE member (id SERIAL PRIMARY KEY, first_name TEXT NOT NULL);
CREATE TABLE member_team (
    id SERIAL PRIMARY KEY,
    member_id INTEGER NOT NULL REFERENCES member(id),
    team_id INTEGER NOT NULL
);
CREATE VIEW visible_member AS SELECT id FROM member;
```

```sql
SELECT visible_member.id AS member_id, member_team.team_id
FROM member
LEFT JOIN member_team ON member_team.member_id = member.id
JOIN visible_member ON visible_member.id = member_team.member_id
-- inferred: tuple[int, int]
```

## LEFT JOIN from a one-row derived table

`(SELECT 1 AS id)` is the preserved side, and `member` can still miss.

`schema.sql`:

```sql
CREATE TABLE member (id SERIAL PRIMARY KEY, first_name TEXT NOT NULL);
```

```sql
SELECT subselect1.id, member.first_name
FROM (SELECT 1 AS id) AS subselect1
LEFT JOIN member ON subselect1.id = member.id
-- inferred: tuple[int, str | None]
```

## Arithmetic on a left-joined column

Subtraction is strict, so it is NULL wherever `prev_week` was null-extended.

`schema.sql`:

```sql
CREATE TABLE songs (id SERIAL PRIMARY KEY, artist TEXT NOT NULL, title TEXT NOT NULL);
CREATE TABLE top50 (
    id SERIAL PRIMARY KEY,
    week INTEGER NOT NULL,
    song_id INTEGER NOT NULL REFERENCES songs(id),
    position INTEGER NOT NULL
);
```

```sql
SELECT this_week.week AS cur_week,
       this_week.position AS cur_position,
       prev_week.position AS prev_position,
       (prev_week.position - this_week.position) AS delta,
       songs.artist, songs.title
FROM top50 AS this_week
JOIN songs ON songs.id = this_week.song_id
LEFT JOIN top50 AS prev_week
  ON prev_week.song_id = this_week.song_id AND prev_week.week = this_week.week - 1
WHERE this_week.week = 15
ORDER BY this_week.week, this_week.position
-- inferred: tuple[int, int, int | None, int | None, str, str]
```

## CASE over two left-joined columns

Both branches read optional relations, so the `CASE` can be NULL; columns reached through
inner joins stay non-null.

`schema.sql`:

```sql
CREATE TABLE workspace (id UUID PRIMARY KEY, title TEXT NOT NULL);
CREATE TABLE data_set_metadata (
    data_set_id UUID PRIMARY KEY,
    owner_id UUID,
    workspace_id UUID NOT NULL REFERENCES workspace(id)
);
CREATE TABLE container_data_set_bridge (
    container_id UUID NOT NULL,
    data_set_id UUID NOT NULL,
    PRIMARY KEY (container_id, data_set_id)
);
CREATE TABLE platform (id UUID PRIMARY KEY, data_set_id UUID);
```

```sql
SELECT m.data_set_id,
       CASE WHEN b.container_id IS NOT NULL THEN b.container_id ELSE p.id END AS reference_id,
       w.title AS workspace_name
FROM data_set_metadata m
JOIN workspace w ON w.id = m.workspace_id
LEFT JOIN container_data_set_bridge b ON b.data_set_id = m.data_set_id
LEFT JOIN platform p ON p.data_set_id = m.data_set_id
-- inferred: tuple[uuid.UUID, uuid.UUID | None, str]
```

## SELECT DISTINCT across inner joins

`DISTINCT` and inner joins keep each column's declared nullability.

`schema.sql`:

```sql
CREATE TABLE prompt_item (id INTEGER PRIMARY KEY, model TEXT NOT NULL);
CREATE TABLE category (id INTEGER PRIMARY KEY, name TEXT NOT NULL, slug TEXT);
CREATE TABLE prompt_categories (
    prompt_id INTEGER NOT NULL REFERENCES prompt_item(id),
    category_id INTEGER NOT NULL REFERENCES category(id)
);
```

```sql
SELECT DISTINCT pi.model, c.name AS category_name, c.slug AS category_slug
FROM prompt_item pi
JOIN prompt_categories pc ON pi.id = pc.prompt_id
JOIN category c ON pc.category_id = c.id
ORDER BY pi.model ASC, c.name
-- inferred: tuple[str, str, str | None]
```
