# Named databases in one project

> **Status:** stage 1 (`databases`, picked by path) in review. Stages 2 and 3 to follow.

Tracks [#912](https://github.com/jirikuncar/sqlty/issues/912), Case 2: one package queries
several databases, each with its own dialect and schema. Case 1, the monorepo, shipped as
`extend` in [#974](https://github.com/jirikuncar/sqlty/pull/974).

## Decisions

From the issue:

- Named databases inherit the top-level keys; `rules` and `types` merge key by key.
- A query names its database in its marker, `--sql sqlty:db=<name>`, and a `.sql` file in a
  header of the same spelling, parsed by one function.
- The typed per-database alias (`AnalyticsSQL[R]`) is a separate issue.

## Stages

| Stage | What | Where |
|-------|------|-------|
| 1 | `[tool.sqlty.databases.NAME]`: `dialect`, `schema`, `queries`, `query-format`, `rules`, `types`, `functions`, picked per file by `include`/`exclude` globs or by `queries` | `config.rs`, `_config.py`, `SettingsResolver.for_file` |
| 2 | `--sql sqlty:db=<name>` in a string and `-- sqlty: db=<name>` in a `.sql` file pick the database of one query | `sqlty-python` marker parsing, per-query settings in `check_source`/`rewrite_source` |
| 3 | A helper listed in a database's `functions` checks its calls against that database, in any file | `_helpers.py`, `configured_helpers` |

## Stage 1

- A file takes the first database whose `queries` name it, then the first whose `include`
  matches and `exclude` does not; otherwise the top-level settings, the default database.
- A database name is `[A-Za-z0-9_-]+`, and `default` is reserved, so stage 2's marker can
  name any database and `db=default` the top level.
- `--dialect` and `--schema` apply to the default database and to a database that does not
  set them.
- `extend` merges `databases` by name and rebases each database's `schema` and `queries`.
- Gaps: the magic codec decodes with the top-level settings
  ([#985](https://github.com/jirikuncar/sqlty/issues/985)). `breaking` routes a file
  through the same resolver.

## Stage 2

- The marker today must be followed by a newline (`checker.rs`, `MARKER_RE`); everything
  after `--sql ` on its line is already a SQL comment, so `--sql sqlty:db=analytics\nSELECT`
  is valid SQL and only the recognition changes.
- `check_source` takes one schema per call. A file mixing databases needs a schema per query:
  either `check_source` takes a map of database name to settings, or the host splits the
  queries of a file by marker and checks each group.
- An unknown name is a diagnostic with the closest name suggested.
