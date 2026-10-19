---
name: sqlty-ci
description: Set up or repair sqlty in pre-commit and CI. Use when adding the sqlty-rewrite or sqlty-check pre-commit hooks, writing a GitHub Actions job that runs sqlty rewrite --check, sqlty check and sqlty breaking, a CI run fails on a sqlty step, a hook fails to install (Python 3.14, Rust toolchain), promoting stale-type or other rules to errors, or keeping committed sources free of SQL[T] wrappers.
---

# sqlty in CI

Run sqlty where it blocks bad queries: before a commit and on every pull
request. Each command exits 1 on what it guards:

| Command | Fails when |
|---------|------------|
| `sqlty rewrite --check .` | a `--sql` string is not wrapped yet |
| `sqlty check .` | a rule with severity `error` fires |
| `sqlty breaking --against REF` | the schema change breaks the queries of either revision |
| `sqlty strip --check .` | a file has `SQL[T]` wrappers, when you commit plain strings |

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty --help` disagree, trust the command.

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

- `fetch-depth: 0` is required: with the default shallow clone, `sqlty breaking`
  can't find the base branch and exits 2.
- `--output-format github` prints each diagnostic as an annotation on its line
  of the pull request's diff.
- sqlty must be a dependency of the project for `uv sync` to install it.

## Pre-commit

```yaml
# .pre-commit-config.yaml
default_language_version:
  python: python3.14  # sqlty requires Python >= 3.14
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: v0.2.0
    hooks:
      - id: sqlty-rewrite
      - id: sqlty-check
```

| Failure | Fix |
|---------|-----|
| the hook environment can't install sqlty | keep `default_language_version` at Python 3.14 or newer |
| the install compiles Rust, or `cargo` is missing | install a Rust toolchain, or use the local hooks below |
| `sqlty-rewrite` fails the commit | it rewrote files: review and stage them |

Local hooks reuse the `sqlty` of your environment and need no build:

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-rewrite
        name: sqlty rewrite
        entry: uv run sqlty rewrite .
        language: system
        pass_filenames: false
        types: [python]
      - id: sqlty-check
        name: sqlty check
        entry: uv run sqlty check .
        language: system
        pass_filenames: false
        types: [python]
```

## Fail on drift

`narrow-type` (a `SQL[tuple[...]]` that doesn't admit the rows its query now
returns, such as `str` for a column made nullable) is an error. `stale-type` (a
`SQL[tuple[...]]` only wider than the rows, such as `str | None` for a column made
`NOT NULL`) and `stale-rewrite` (an unwrapped `--sql` string) are warnings by
default, so `sqlty check` exits 0 on them. Promote what you want CI to enforce:

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.rules]
stale-type = "error"
untyped-query = "error"
```

`sqlty check --error-on-warning` fails on every warning instead. Keep `schema`
current: regenerate it from the database in the same pull request as the
migration (`pg_dump --schema-only`, `sqlite3 app.db .schema`,
`mysqldump --no-data`), or point `schema` at the migrations directory.

## Monorepos

Each file uses the nearest `pyproject.toml` above it that has a `[tool.sqlty]`
table, so one `sqlty check .` covers several projects. A nested table replaces
the root one; they are not merged.

## Reference

- [CI and pre-commit](https://jirikuncar.github.io/sqlty/recipes/ci/)
- [Configuration](https://jirikuncar.github.io/sqlty/reference/configuration/)
- [Rules](https://jirikuncar.github.io/sqlty/reference/rules/)
