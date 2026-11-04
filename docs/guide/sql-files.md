# Queries in `.sql` files

```toml
[tool.sqlty]
schema = "schema.sql"
queries = ["sql/**/*.sql"]
```

`schema.sql`:

```sql
CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL);
```

`sqlty check` reads the named queries of the `.sql` files `queries` matches, and
runs the same rules on them as on queries in Python strings. Projects that keep
SQL in files, loaded with [aiosql](https://nackjicholson.github.io/aiosql/defining-sql-queries.html),
[PugSQL](https://pugsql.org) or shared with [sqlc](https://docs.sqlc.dev/en/latest/reference/query-annotations.html),
get their misspelled columns and unknown tables reported:

```sql
-- name: get-user-by-email^
-- error: [unknown-column] "column `emial` does not exist in `users`; did you mean `email`?"
SELECT id, name, emial FROM users WHERE email = :email;
```

```text
sql/users.sql:3: [unknown-column] column `emial` does not exist in `users`; did you mean `email`?
```

`queries` is kept apart from `schema`, so a migration in `schema` is never read as a query. It
takes files, directories (every `*.sql` under them) and globs, relative to the
configuration file, as `schema` does. `sqlty check sql/` checks the query files
under `sql/`; a `.sql` file `queries` does not match is not checked.

## Headers

Each file's first header picks its format; `query-format` fixes one instead. A
query runs from the line after its header to the next header. Text before the
first header is no query.

| `query-format` | Header | Query name |
|----------------|--------|------------|
| `aiosql` | `-- name: get-user^` | `get_user`: `-` reads as `_` |
| `sqlc` | `-- name: GetUser :one` | `GetUser` |
| `pugsql` | `-- :name get_user :one` | `get_user` |
| `file` | none | the file name: one query per file, as Squirrel and Prisma TypedSQL keep them |
| `auto` (default) | the first header found | |

A query of several statements, such as an aiosql script (`-- name: reset#`),
has each statement checked. Placeholders are read as the engine reads them in
Python strings: `:name` and `$1` are parameters.

```sql
-- name: GetUser :one
-- error: [unknown-column] "column `nme` does not exist in `users`; did you mean `name`?"
SELECT id, nme FROM users WHERE id = $1;
```

## What is checked

The [name rules](../reference/rules.md#name-rules) (`unknown-table`,
`unknown-column`, `ambiguous-column`, ...) and
[`unparsed-query`](../reference/rules.md#unparsed-query), with the severities
`[tool.sqlty.rules]` and `overrides` give them. A file has no row types, so the
rules that compare one with the query don't apply.

`-- sqlty: ignore` and `-- sqlty: ignore[rule]` silence the diagnostics of their
line, and `unused-ignore` reports one that silences nothing:

```sql
-- name: legacy
-- error:
SELECT id, nickname FROM users; -- sqlty: ignore[unknown-column]
```

`--output-format json`, `github` and `sarif` report them like any other
diagnostic, with the line and column in the `.sql` file.

`sqlty breaking` compares the query files of both revisions, as it compares the
Python files: see [Breaking schema changes](breaking-changes.md#queries-in-sql-files).

`sqlty server` publishes the diagnostics of a `.sql` buffer that `queries` names,
as you type. The VS Code extension sends it `sql` files; in Neovim, add `"sql"`
to the client's [`filetypes`](../recipes/editors.md#neovim).
