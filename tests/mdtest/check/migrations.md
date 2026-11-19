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
