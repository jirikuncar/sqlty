# CI and pre-commit

Run sqlty where it blocks bad queries: before a commit and on every pull
request. `sqlty rewrite --check` and `sqlty strip --check` exit 1 when files
would change; `sqlty check` exits 1 when a rule with severity `error` fires.
Any CI system can use them.

## Pre-commit

The sqlty repository publishes two hooks. Both receive the staged Python
files as arguments:

| Hook id | Runs |
|---------|------|
| `sqlty-rewrite` | `sqlty rewrite <files>` |
| `sqlty-check` | `sqlty check <files>` |

```yaml
# .pre-commit-config.yaml
default_language_version:
  python: python3.15  # sqlty requires Python >= 3.15
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: <commit SHA>  # pre-commit autoupdate --bleeding-edge --freeze
    hooks:
      - id: sqlty-rewrite
      - id: sqlty-check
```

No release includes these hooks yet (`v0.0.1.dev1` predates them), so pin
`rev` to a commit: `pre-commit autoupdate --bleeding-edge --freeze` fills in
the current one. pre-commit builds the hook environment with the Python it
runs on unless told otherwise, and sqlty needs 3.15: keep
`default_language_version`, or set `language_version: python3.15` on each
hook. pre-commit installs sqlty from source into that environment, which
compiles the Rust extension, so the first run needs a Rust toolchain
(`cargo`) and takes a few minutes. Pass options with `args`, for example
`args: [--dialect, sqlite]`.

`sqlty-check` fails the commit only for rules with severity `error`; drift
such as [`stale-type`](../reference/rules.md#stale-type) is a warning unless
you promote it (see [below](#fail-on-drift-and-mismatches)). The `sqlty` hook
id is a deprecated alias of `sqlty-rewrite`: it rewrites the staged files and
no longer generates stubs.

To skip the build or reuse the `sqlty` already installed in your
environment, use local hooks instead:

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

Each file uses the nearest `pyproject.toml` above it that has a `[tool.sqlty]`
table, so one command covers several projects with their own settings. In a
monorepo, a package `pyproject.toml` without `[tool.sqlty]` is skipped and its
files use the root config; one with its own table replaces the root config
entirely (tables are not merged).

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

`stale-rewrite` reports exactly the strings `rewrite` would wrap, so promoting
it to `error` is equivalent to `rewrite --check`.

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
