# DuckDesk: shipping on a Friday with a rubber duck

A 30-minute live-coding talk. We build DuckDesk, a startup that rents rubber
ducks to developers by the hour, and deploy it on every push, from a laptop.
sqlty types the SQL, Logfire traces it, prek is the whole pipeline (commit,
merge and push), the VS Code extension fixes what a merge broke, and Docker
runs two versions side by side while a migration rolls out.

[TRANSCRIPT.md](TRANSCRIPT.md) is what to say while typing.

## Before the talk

- [uv](https://docs.astral.sh/uv/), git, Docker with Compose
- VS Code with the [sqlty extension](https://marketplace.visualstudio.com/items?itemName=jirikuncar.sqlty),
  with the app's `.venv` selected as the Python interpreter
- Optional: a Logfire project (`logfire auth`), and `LOGFIRE_TOKEN` exported for the
  containers

The scripts install `sqlty>=0.3.0`, the first release with `sqlty server`, which
the extension runs. Pull `postgres:17-alpine`, `debian:trixie-slim` and `ghcr.io/astral-sh/uv:0.11` before going on
stage: conference Wi-Fi is not a deployment platform.

```bash
export DUCKDESK_DIR=~/duckdesk      # where the app is built (default)
./steps/run.sh 8                    # dry run of the whole story, then:
./steps/run.sh 0                    # reset to an empty project before going on stage
```

`00-setup.sh` also removes the containers, the database volume and the
`duckdesk-origin.git` remote of an earlier run.

## Steps

Each script runs the step it is named after, on top of the one before, and
commits the result. If live typing goes wrong, run `./steps/run.sh N`: it
rebuilds the app from scratch up to step N. `SOAK` sets how many seconds two
versions run side by side (5 in the scripts; 15 reads better on stage).

| Step | Script | Minutes | What the audience sees |
|------|--------|---------|------------------------|
| 0 | `00-setup.sh` | 0-2 | `uv init`, dependencies |
| 1 | `01-migrations.sh` | 2-5 | Postgres in Compose; three tables as migrations; a 20-line migrator |
| 2 | `02-queries.sh` | 5-9 | `sqlty rewrite` writes row and parameter types, `INSERT`/`UPDATE ... RETURNING` included |
| 3 | `03-api.sh` | 9-12 | The request body is the query's parameters; `bind`, ty, tests |
| 4 | `04-logfire.sh` | 12-14 | Request and query spans, tagged with the version |
| 5 | `05-prek.sh` | 14-17 | Tests pass, prek still rejects the commit |
| 6 | `06-deploy.sh` | 17-21 | `git push` builds an image, migrates and rolls it out |
| 7 | `07-bobs-merge.sh` | 21-25 | Two green branches; prek blocks the merge; Quick Fix |
| 8 | `08-rolling-deploy.sh` | 25-29 | `sqlty breaking` stops the deploy; expand, then contract |
