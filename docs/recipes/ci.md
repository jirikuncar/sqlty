---
description: How to run sqlty in pre-commit and CI, such as GitHub Actions, so a pull request with a query that no longer matches the schema fails the build.
---

# CI and pre-commit

Run sqlty where it blocks bad queries: before a commit and on every pull
request. `sqlty rewrite --check` and `sqlty strip --check` exit 1 when files
would change; `sqlty check` exits 1 when a rule with severity `error` fires.
Any CI system can use them.

## Pre-commit

The sqlty repository publishes two hooks. Both receive the staged Python, SQL
and TOML files as arguments:

| Hook id | Runs |
|---------|------|
| `sqlty-rewrite` | `sqlty rewrite <files>` |
| `sqlty-check` | `sqlty check <files>` |

```yaml
# .pre-commit-config.yaml
default_language_version:
  python: python3.14  # sqlty requires Python >= 3.14
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: v0.5.0
    hooks:
      - id: sqlty-rewrite
      - id: sqlty-check
```

`pre-commit autoupdate` moves `rev` to the latest release. pre-commit
builds the hook environment with the Python it runs on unless told otherwise,
and sqlty needs 3.14 or newer: keep `default_language_version`, or set
`language_version` on each hook. pre-commit installs sqlty from source into that environment, which
compiles the Rust extension, so the first run needs a Rust toolchain
(`cargo`) and takes a few minutes. Pass options with `args`, for example
`args: [--dialect, sqlite]`.

`sqlty-check` fails the commit only for rules with severity `error`. Drift that
can break the code, [`narrow-type`](../reference/rules.md#narrow-type), is one by
default; an annotation only wider than the rows,
[`stale-type`](../reference/rules.md#stale-type), is a warning unless you
promote it (see [below](#fail-on-drift-and-mismatches)). The `sqlty` hook
id is a deprecated alias of `sqlty-rewrite`: it rewrites the staged files.

!!! note "A schema change checks the whole project"
    A changed schema or config file can break queries in files you didn't touch.
    When one of the files is the `schema` of a project, or the `sqlty.toml`,
    `pyproject.toml` or `Cargo.toml` that configures it, sqlty runs on every
    Python file of that project, as `sqlty check .` would. Other `.sql` and
    `.toml` files are ignored.

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
        types_or: [python, sql, toml]
      - id: sqlty-check
        name: sqlty check
        entry: sqlty check .
        language: system
        pass_filenames: false
        types_or: [python, sql, toml]
```

`sqlty rewrite` edits files in place, which makes pre-commit fail the commit
and leave the rewritten files for you to stage. Use `uv run sqlty ...` as the
entry if sqlty lives in a uv-managed environment.

Each file uses the nearest `sqlty.toml` above it, or `pyproject.toml` or `Cargo.toml`
with a sqlty table, so one command covers several projects with their own settings. In a
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
By default that covers row type mismatches, names the schema doesn't have,
`SELECT *` without a schema, and annotations that don't admit the rows
([`narrow-type`](../reference/rules.md#narrow-type)); annotations only wider than
the rows ([`stale-type`](../reference/rules.md#stale-type)) and unwrapped strings
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
        with:
          fetch-depth: 0  # sqlty breaking reads the base branch
      - uses: astral-sh/setup-uv@v7
      - run: uv sync
      - run: uv run sqlty rewrite --check .
      - run: uv run sqlty check --output-format github .
      - run: uv run sqlty breaking --output-format github --against origin/${{ github.base_ref }}
```

Each step catches something the others don't:

| Step | Fails when |
|------|------------|
| `sqlty rewrite --check` | a `--sql` string is not wrapped yet |
| `sqlty check` | a rule with severity `error` fires: a row type mismatch, an unknown table or column, or an annotation that doesn't admit the rows ([`narrow-type`](../reference/rules.md#narrow-type)) by default; see [Fail on drift and mismatches](#fail-on-drift-and-mismatches) |
| `sqlty breaking` | the pull request's schema breaks the base branch's queries, or its queries break on the base branch's schema: a dropped column, a column made nullable |

!!! note "A wider annotation is only a warning"
    A migration that makes a column nullable leaves `SQL[tuple[str]]` too narrow,
    and the code can now get `None`: `sqlty check` fails with `narrow-type`. A
    migration that makes a column `NOT NULL` leaves `SQL[tuple[str | None]]` wider
    than it needs to be: `stale-type` warns and `sqlty check` exits 0.

`--output-format github` prints each diagnostic, and each breaking change, as a GitHub
Actions annotation, so it shows on its line in the pull request's diff. See [Schema changes in CI](migration.md#schema-changes-in-ci)
for the `--direction` options.

## GitHub code scanning

```yaml
# .github/workflows/sqlty-code-scanning.yml
name: sqlty code scanning
on:
  push:
    branches: [main]
  pull_request:
jobs:
  sqlty:
    runs-on: ubuntu-latest
    permissions:
      security-events: write
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v7
      - run: uv sync
      - run: uv run sqlty check --output-format sarif --exit-zero . > sqlty.sarif
      - uses: github/codeql-action/upload-sarif@v4
        with:
          sarif_file: sqlty.sarif
```

`--output-format sarif` prints a [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
log: each diagnostic with its rule, severity, location and fix edits, and every rule with
a link to its documentation. GitHub code scanning keeps the findings across runs, so you
can triage and dismiss them in the **Security** tab. GitLab, Azure DevOps and the VS Code
SARIF Viewer read the same file.

`--exit-zero` lets the upload step run when `sqlty check` finds errors. Keep the
`sqlty check` step of [GitHub Actions](#github-actions) to fail the pull request.
`sqlty breaking --output-format sarif` works too, with each finding's `direction` and
`against` in its `properties`.

## Keeping sources stripped

If you commit plain `--sql` strings and rely on the
[magic codec](../guide/magic-codec.md) instead, `sqlty strip --check .` exits
with 1 when a file contains `SQL[T](...)` wrappers.
