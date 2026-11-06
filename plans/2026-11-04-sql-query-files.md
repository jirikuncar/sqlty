# Check named queries in `.sql` files

> **Status:** stages 1-4 merged ([#906](https://github.com/jirikuncar/sqlty/pull/906),
> [#908](https://github.com/jirikuncar/sqlty/pull/908), [#913](https://github.com/jirikuncar/sqlty/pull/913),
> [#915](https://github.com/jirikuncar/sqlty/pull/915)); the gaps are tracked in
> [#945](https://github.com/jirikuncar/sqlty/issues/945): `select-star` merged in
> [#965](https://github.com/jirikuncar/sqlty/pull/965), sqlc/aiosql placeholders in review.

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

## Stage 2

- A revision's files include the query files its `queries` names; a snapshot keeps them, with
  `query_format` in their settings (an older snapshot reads `auto`).
- `breaking_query_file` reports what a query file gets against the other schema and not
  against its own, as `breaking_source` does for Python.

## Stage 3

- `sqlty server` opens `sql` documents too. One that `queries` names (a glob match, so an
  unsaved buffer counts) gets the diagnostics of `sqlty check`; any other gets none.
  Completion, hover, highlighting and fix-all stay Python-only.
- The VS Code extension sends `sql` documents; the Neovim recipe adds `"sql"` to `filetypes`.

## Stage 4

- `sqlty check -v` prints `name(params) -> row` for each named query, after the diagnostics,
  in text output: `None` for a query without rows, `unknown` for one whose columns are not
  all typed. A query of several statements is typed by its first.

## Gaps

- sqlc's `@name`, `sqlc.arg(name)` and `sqlc.narg(name)`, and aiosql's `:obj.attr`, are
  rewritten to `:name` (`:obj_attr`) before the engine reads a query, padded to their length
  so spans hold. `sqlc.slice(name)` and `sqlc.embed(table)` are not read.
- `select-star` is reported in query files, but `check --fix` does not expand the star there.
