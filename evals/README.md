# Agent evals

These cases measure whether the sqlty Agent Skills change what a coding agent
writes. Each case starts from the same small project
([`_project/shop.sh`](_project/shop.sh): `users`, `orders` and `order_items`,
psycopg, migrations in `db/migrations`, sqlty in `.venv`, one git commit) and
asks for one common change in the words a developer would use. No prompt names
a skill or a sqlty command.

| Case | Asked for | Passes when the agent |
| --- | --- | --- |
| `new-query` | a query returning a user's email and name | wraps it as `SQL[tuple[str, str \| None]...]` with `--sql` on its own line, via `sqlty rewrite` |
| `name-row-type` | a `TypedDict` for the `USER` rows | writes `name: str \| None` and `email: str`, runs `sqlty check` |
| `revenue-double-count` | a fix for revenue that is too high | aggregates `order_items` per order first; no `DISTINCT`, no `sqlty: ignore` |
| `rename-column` | `users.name` renamed to `full_name` | adds `full_name` and keeps `name` (expand and contract), runs `sqlty breaking` |
| `ci-job` | a GitHub Actions workflow for SQL | runs `rewrite --check`, `check` and `breaking --against` with `fetch-depth: 0` |
| `adopt` | sqlty set up in a project without it | configures `schema = "db/migrations"` and types both queries |
| `display-name` | a helper with no SQL | a control: the skills must not get in the way |

`tool_used: Skill` graders show whether a skill loaded; they are not scored.

Run with and without the plugin (the runner adds the baseline arm on its own):

```bash
evals/run.sh
SQLTY_EVAL_SPEC=sqlty==0.6.0 evals/run.sh --case new-query --runs 1
```

[`run.sh`](run.sh) copies the plugin to a temporary directory, since the runner
refuses one with hard links (cargo's `rust/target`, uv's `.venv`) or symbolic
links, installs `SQLTY_EVAL_SPEC` (PyPI's latest `sqlty` by default) into
`~/.cache/sqlty-evals/venv` (the agent's sandbox hides `/tmp`), and puts it first on `PATH` and runs `claude plugin eval --scaffold`. Each
case's `scaffold.sh` links the project's `.venv` to it: the runner gives scaffolds
an empty `HOME` and no network settings.
Results land in `evals/results/`.
