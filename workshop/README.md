# DuckDesk: shipping on a Friday with a rubber duck

A 30-minute live-coding talk. We build DuckDesk, a startup that rents rubber
ducks to developers by the hour, and deploy it on every push. Along the way
sqlty types the SQL, Logfire traces it, prek blocks a bad commit, and the VS Code
extension fixes a bug that two green pull requests made together.

[TRANSCRIPT.md](TRANSCRIPT.md) is what to say while typing.

## Before the talk

- [uv](https://docs.astral.sh/uv/), git, VS Code with the
  [sqlty extension](https://marketplace.visualstudio.com/items?itemName=jirikuncar.sqlty),
  with the app's `.venv` selected as the Python interpreter
- Optional, for the deploy: `gh`, `flyctl`, and a Logfire project with a write token

The scripts install `sqlty>=0.3.0`, the first release with `sqlty server`, which
the extension runs. To try an unreleased sqlty, set
`SQLTY_SPEC="sqlty @ git+https://github.com/jirikuncar/sqlty"` (needs Rust).

```bash
export DUCKDESK_DIR=~/duckdesk      # where the app is built (default)
./steps/run.sh 8                    # dry run of the whole story, then:
./steps/run.sh 0                    # reset to an empty project before going on stage
```

For the deploy, push the app to GitHub once and follow what `06-cicd.sh` prints.

## Steps

Each script runs the step it is named after, on top of the one before, and
commits the result. If live typing goes wrong, run `./steps/run.sh N`: it
rebuilds the app from scratch up to step N.

| Step | Script | Minutes | What the audience sees |
|------|--------|---------|------------------------|
| 0 | `00-setup.sh` | 0-2 | `uv init`, dependencies |
| 1 | `01-schema.sh` | 2-4 | Three tables: `ducks`, `developers`, `consultations` |
| 2 | `02-queries.sh` | 4-9 | `sqlty rewrite` writes the row types from the schema |
| 3 | `03-api.sh` | 9-12 | FastAPI endpoints and two tests |
| 4 | `04-logfire.sh` | 12-15 | Request and query spans, a custom log |
| 5 | `05-prek.sh` | 15-19 | Tests pass, prek still rejects the commit |
| 6 | `06-cicd.sh` | 19-22 | GitHub Actions: the same hooks, then `flyctl deploy` |
| 7 | `07-bobs-merge.sh` | 22-27 | Two green branches merge into a red main; Quick Fix |
| 8 | `08-breaking.sh` | 27-30 | `sqlty breaking`: the rename still breaks a rolling deploy |
