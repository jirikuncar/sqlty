# CI and pre-commit

Run sqlty where it blocks bad queries: before a commit and on every pull
request. `sqlty rewrite --check` and `sqlty strip --check` exit 1 when files
would change; `sqlty check` exits 1 when a rule with severity `error` fires.
Any CI system can use them.

## Pre-commit

Use local hooks that call the `sqlty` installed in your environment:

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-rewrite
        name: sqlty rewrite
        entry: sqlty rewrite .
        language: system
        pass_filenames: false
        types: [python]
      - id: sqlty-check
        name: sqlty check
        entry: sqlty check .
        language: system
        pass_filenames: false
        types: [python]
```

`sqlty rewrite` edits files in place, which makes pre-commit fail the commit
and leave the rewritten files for you to stage. Use `uv run sqlty ...` as the
entry if sqlty lives in a uv-managed environment.

Each file uses the nearest `pyproject.toml` above it, so one invocation can
cover several projects with their own `[tool.sqlty]` settings.

## Fail on unwrapped queries

`sqlty rewrite --check` changes nothing and exits with 1 if any file would
be rewritten:

```bash
$ sqlty rewrite --check .
Would rewrite queries.py
1 file(s) would be rewritten
```

`--diff` prints the changes as a unified diff instead. Add `--untyped
sentinel` (or set `untyped = "sentinel"`) to also require wrapping queries
whose row type is unknown; see [Untyped queries](../guide/untyped.md).

## Fail on drift and mismatches

`sqlty check` exits with 1 only when a rule with severity `error` fires.
By default that covers row type mismatches and `SELECT *` without a schema;
drift ([`stale-type`](../reference/rules.md#stale-type)) and unwrapped strings
([`stale-rewrite`](../reference/rules.md#stale-rewrite)) are warnings. Promote
the rules you want to enforce:

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "error"
untyped-query = "error"
```

`stale-rewrite` also fires for `--sql` strings that `rewrite` never wraps
(statements without rows, f-strings), so prefer `rewrite --check` over
promoting it.

Keep the schema file current, for example by regenerating it from the
database in the same pull request as the migration
(`pg_dump --schema-only`, `sqlite3 app.db .schema`,
`mysqldump --no-data`).

## GitHub Actions

```yaml
# .github/workflows/sqlty.yml
name: sqlty
on: [pull_request]
jobs:
  sqlty:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v7
      - run: uv sync
      - run: uv run sqlty rewrite --check .
      - run: uv run sqlty check .
```

## Generated stubs

There is no command that checks [generated stubs](../guide/stubs.md). To keep
them current, run the test suite with `SQLTY_AUTO_GENERATE=1` and fail if a
stub changed:

```bash
SQLTY_AUTO_GENERATE=1 uv run pytest
git diff --exit-code -- '*.pyi'
```

## Keeping sources stripped

If you commit plain `--sql` strings and rely on the
[magic codec](../guide/magic-codec.md) instead, `sqlty strip --check .` exits
with 1 when a file contains `SQL[T](...)` wrappers.
