# Check named queries in `.sql` files

> **Status:** stage 1 (`check`) in review. Stages 2 to 4 to follow.

Tracks [#896](https://github.com/jirikuncar/sqlty/issues/896): projects that keep SQL in `.sql`
files (aiosql, PugSQL, sqlc) get no checks. The engine splits and checks the files
(`sqlty_python::queries`); the Python host finds them and applies `-- sqlty: ignore`.

## Stages

| Stage | What | Where |
|-------|------|-------|
| 1 | `queries` and `query-format` keys; `sqlty check` reports name rules and `unparsed-query` in query files, with `-- sqlty: ignore` and every output format | `sqlty-python/src/queries.rs`, `_queries.py`, `_cli.py` |
| 2 | `sqlty breaking` includes query files on both sides | `_breaking.py` |
| 3 | `sqlty server` publishes diagnostics for `sql` documents `queries` matches | `sqlty-lsp`, `_lsp_bridge.py` |
| 4 | `--verbose` lists the row and parameter types of each named query | `_cli.py` |

## Stage 1

- `queries` takes files, directories and globs relative to the configuration file, as
  `schema` does, and only those files are read: a migration is never a query.
- `query-format`: `auto` (default) picks each file's format from its first header, `file`
  when it has none. A query runs from the line after its header to the next header; each of
  its statements is checked.
- Answers to the issue's open questions: `auto` applies only to the files `queries` names;
  Jinja and dbt templates are not rendered (a template parses as `unparsed-query`).

## Gaps

- sqlc's `@name` and `sqlc.arg(name)` placeholders are not read as parameters, and aiosql's
  `:obj.attr` attribute access is read as a column of a table `obj`.
- `select-star` is not reported in query files.
