# Migrations

`against.sql`:

```sql
CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT, team_id BIGINT);
```

## A column with an inline unique constraint

```sql migration
SET lock_timeout = '1s';
-- error: [missing-concurrently] "the constraint builds its index under an ACCESS EXCLUSIVE lock on `users`"
-- error: [add-required-column]
ALTER TABLE users ADD COLUMN handle TEXT PRIMARY KEY;
```

## A table the baseline has is not new

```sql migration
SET lock_timeout = '1s';
CREATE TABLE IF NOT EXISTS users (id BIGINT);
-- error: [missing-concurrently]
CREATE INDEX users_email ON users (email);
```

## The baseline in another case

```sql migration
-- error:
create table users (id bigint primary key, email text, team_id bigint);
```

## A lock timeout set for one transaction

```sql migration
BEGIN;
SET LOCAL lock_timeout = '1s';
ALTER TABLE users ADD COLUMN bio TEXT;
COMMIT;
-- error: [missing-lock-timeout]
ALTER TABLE users ADD COLUMN about TEXT;
```

## A pg_dump schema

```sql migration
--
-- PostgreSQL database dump
--

CREATE INDEX users_email ON public.users USING btree (email);
-- error:
ALTER TABLE ONLY public.users ADD CONSTRAINT users_team FOREIGN KEY (team_id) REFERENCES public.users(id);
```

## A table copied with `LIKE` is new

```sql migration
-- error:
CREATE TABLE users_archive (LIKE users INCLUDING DEFAULTS INCLUDING CONSTRAINTS);
CREATE INDEX users_archive_email ON users_archive (email);
```

## Rows copied into a new table

```sql migration
SET lock_timeout = '1s';
CREATE TABLE users_archive (LIKE users INCLUDING ALL);
INSERT INTO users_archive SELECT * FROM users;
-- error: [missing-concurrently] "`CREATE INDEX` blocks writes to `users_archive`"
CREATE INDEX users_archive_email ON users_archive (email);
```

## Attaching a partition

```sql migration
SET lock_timeout = '1s';
-- error: [constraint-scan] "`ATTACH PARTITION` scans `events_09` for rows outside its bounds"
ALTER TABLE events ATTACH PARTITION events_09 FOR VALUES FROM ('2026-09-01') TO ('2026-10-01');
ALTER TABLE events_10 ADD CONSTRAINT events_10_at CHECK (at IS NOT NULL) NOT VALID;
ALTER TABLE events_10 VALIDATE CONSTRAINT events_10_at;
ALTER TABLE events_10 ADD CONSTRAINT events_10_bounds CHECK (at >= '2026-10-01' AND at < '2026-11-01') NOT VALID;
ALTER TABLE events_10 VALIDATE CONSTRAINT events_10_bounds;
ALTER TABLE events ATTACH PARTITION events_10 FOR VALUES FROM ('2026-10-01') TO ('2026-11-01');
```

## A new partition scans the default partition

```sql migration
SET lock_timeout = '1s';
-- error: [constraint-scan] "`ATTACH PARTITION` scans `events_rest`"
ALTER TABLE events ATTACH PARTITION events_rest DEFAULT;
-- error: [constraint-scan] "a new partition of `events` scans its default partition `events_rest`"
CREATE TABLE events_10 PARTITION OF events FOR VALUES FROM ('2026-10-01') TO ('2026-11-01');

## TRUNCATE CASCADE of a table no foreign key references

```sql migration
SET lock_timeout = '1s';
-- error:
TRUNCATE users CASCADE;
```

## A new enum value in a column list that doesn't name its type

```sql migration
BEGIN;
SET LOCAL lock_timeout = '1s';
CREATE TYPE mood AS ENUM ('ok');
ALTER TABLE users ADD COLUMN mood mood;
COMMIT;
BEGIN;
ALTER TYPE mood ADD VALUE 'meh';
-- error:
UPDATE "users" SET email = 'meh' WHERE id = 1;
COMMIT;
```

## A new enum value in a column of another table

```sql migration
BEGIN;
SET LOCAL lock_timeout = '1s';
CREATE TYPE feeling AS ENUM ('ok');
CREATE TABLE feelings (feeling feeling);
COMMIT;
BEGIN;
ALTER TYPE feeling ADD VALUE 'meh';
-- error:
UPDATE users feeling SET email = 'meh' WHERE feeling.id = 1;
COMMIT;
```

## Reindexing a table the migration creates

```sql migration
SET lock_timeout = '1s';
CREATE TABLE drafts (id BIGINT PRIMARY KEY);
-- error:
REINDEX TABLE drafts;
```

## A NOT NULL check alone does not bound a partition

```sql migration
SET lock_timeout = '1s';
ALTER TABLE events_11 ADD CONSTRAINT events_11_at CHECK (at IS NOT NULL) NOT VALID;
ALTER TABLE events_11 VALIDATE CONSTRAINT events_11_at;
-- error: [constraint-scan] "`ATTACH PARTITION` scans `events_11`"
ALTER TABLE events ATTACH PARTITION events_11 FOR VALUES FROM ('2026-11-01') TO ('2026-12-01');
```

## Rows pinned by key are not a backfill

```sql migration
SET lock_timeout = '1s';
-- error:
UPDATE users SET email = 'a@example.com' WHERE id = 1;
DELETE FROM users WHERE id IN (2, 3);
```

## Locks that let writes go on

```sql migration
BEGIN;
SET LOCAL lock_timeout = '1s';
-- error:
LOCK TABLE users IN ACCESS SHARE MODE;
ALTER TABLE events SET (fillfactor = 70);
CREATE TEMP TABLE ids (id BIGINT);
INSERT INTO ids SELECT id FROM users;
UPDATE ids SET id = id + 1;
COMMIT;
```

## Attaching a partition locks the partition

```sql migration
BEGIN;
SET LOCAL lock_timeout = '1s';
-- error: [constraint-scan] "`ATTACH PARTITION` scans `events_09`"
ALTER TABLE events ATTACH PARTITION events_09 FOR VALUES FROM ('2026-09-01') TO ('2026-10-01');
-- error: [locked-backfill] "locks `events_09` against writes"
UPDATE events_09 SET at = at WHERE at BETWEEN '2026-09-01' AND '2026-09-02';
COMMIT;
```

## A replica identity set in the same statement

```sql migration
SET lock_timeout = '1s';
-- error:
ALTER TABLE users DROP CONSTRAINT users_pkey, REPLICA IDENTITY FULL;
```

## Dropping the primary key column

```sql migration
SET lock_timeout = '1s';
-- error: [drop-primary-key] "`users` is left without a replica identity"
ALTER TABLE users DROP COLUMN id;
```

## A pinned key in one branch of OR bounds nothing

```sql migration
SET lock_timeout = '1s';
-- error: [unbatched-backfill]
UPDATE users SET email = '' WHERE id = 1 OR email IS NULL;
```

## A statement that needs two tables at once

```sql migration
BEGIN;
SET LOCAL lock_timeout = '1s';
-- error:
LOCK TABLE users, events;
COMMIT;
```

## Dropping a foreign key locks the table it references

`against.sql`:

```sql
CREATE TABLE teams (id BIGINT PRIMARY KEY);
CREATE TABLE users (id BIGINT PRIMARY KEY, team_id BIGINT REFERENCES teams (id));
```

```sql migration
SET lock_timeout = '1s';
BEGIN;
ALTER TABLE teams ADD COLUMN name TEXT;
-- error: [multi-table-lock] "locks `users` while this transaction still holds `teams`"
ALTER TABLE users DROP COLUMN team_id;
COMMIT;
BEGIN;
ALTER TABLE users ADD COLUMN team BIGINT;
-- error:
ALTER TABLE users DROP CONSTRAINT users_team_id_fkey;
COMMIT;
```
