# Named queries in `.sql` files

A `sql` block with `-- error:` assertions is a `.sql` file of named queries, checked as
`sqlty check` checks the files `queries` matches.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL);
CREATE TABLE blogs (id SERIAL PRIMARY KEY, author_id INTEGER NOT NULL REFERENCES users, title TEXT NOT NULL);
```

## aiosql

`-` in a name reads as `_`; the suffix and parameter list after the name are aiosql's.
Comments between the header and the SQL are skipped.

```sql
-- name: get-user-by-email^
-- error: [unknown-column] "column `emial` does not exist in `users`; did you mean `email`?"
SELECT id, name, emial FROM users WHERE email = :email;

-- name: list-titles(author_id)
-- The titles of an author.
-- error:
SELECT title FROM blogs WHERE author_id = :author_id;

-- name: publish<!
-- error: [unknown-column] "column `body` does not exist in `blogs`"
INSERT INTO blogs (author_id, title, body) VALUES (:author_id, :title, :body) RETURNING id;
```

A script (`#`) holds several statements, each checked:

```sql
-- name: reset#
-- error: [unknown-table] "table `post` is not defined in the schema"
DELETE FROM post;
-- error:
DELETE FROM users;
```

## sqlc

```sql
-- name: GetUser :one
-- error: [unknown-column] "column `nme` does not exist in `users`; did you mean `name`?"
SELECT id, nme FROM users WHERE id = $1;

-- name: ListUsers :many
-- error:
SELECT id, name FROM users ORDER BY name;
```

## PugSQL

```sql
-- :name find_user :one
-- error: [unknown-table] "table `user` is not defined in the schema"
SELECT * FROM user WHERE id = :id
```

## One query per file

A file without headers is one query, named after the file.

```sql
-- error: [ambiguous-column] "column reference `id` is ambiguous"
SELECT id FROM users JOIN blogs ON blogs.author_id = users.id
```

## Parse errors

```sql
-- name: broken
-- error: [unparsed-query] "query does not parse"
SELEC id FROM users;
```

## Suppressions

`-- sqlty: ignore` and `-- sqlty: ignore[rule]` silence their line, as `# sqlty: ignore`
does in Python; one that silences nothing is `unused-ignore`.

```sql
-- name: legacy
-- error:
SELECT id, nickname FROM users; -- sqlty: ignore[unknown-column]

-- name: tidy
-- error: [unused-ignore] "'-- sqlty: ignore' suppresses no diagnostic"
SELECT id FROM users; -- sqlty: ignore
```

## Format

With `query-format`, only that format's headers start a query.

```toml
[tool.sqlty]
schema = "schema.sql"
query-format = "file"
```

```sql
-- name: one
-- error: [unknown-column] "column `nope` does not exist in `users`"
SELECT nope FROM users;
```
