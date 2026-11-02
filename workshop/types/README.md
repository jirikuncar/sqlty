# DuckDesk: the types talk back

A 20-minute live-coding talk about types, the companion of the
[CI/CD talk](../README.md). One developer, one branch, no deploy: the schema
types the SQL, sqlty carries those types into Python, ty follows them to the line
that breaks, and the published prek hooks check every commit.

[TRANSCRIPT.md](TRANSCRIPT.md) is what to say while typing.
[../recording/](../recording/) makes a narrated video of it (`TALK=types`).

## Before the talk

- [uv](https://docs.astral.sh/uv/), git, Docker with Compose, and a Rust toolchain
  (`cargo`): prek builds the published sqlty hook from source, once
- VS Code with the [sqlty extension](https://marketplace.visualstudio.com/items?itemName=jirikuncar.sqlty)
  and the [ty extension](https://marketplace.visualstudio.com/items?itemName=astral-sh.ty)

```bash
export DUCKDESK_DIR=~/duckdesk      # where the app is built (default)
./steps/run.sh 5                    # dry run of the whole story; builds the hooks
./steps/run.sh 0                    # reset to an empty project before going on stage
```

The first run builds the sqlty hook (about 3 minutes); prek caches it, so
`prek prepare-hooks` on stage takes a second. Pull `postgres:17-alpine` before
going on stage.

## Steps

Each script runs the step it is named after, on top of the one before, and
commits the result through the hooks. If live typing goes wrong, run
`./steps/run.sh N`: it rebuilds the app from scratch up to step N.

| Step | Script | Minutes | What the audience sees |
|------|--------|---------|------------------------|
| 0 | `00-setup.sh` | 1:00 | `prek.toml` with the published sqlty and ruff hooks, and ty |
| 1 | `01-schema.sh` | 3:00 | Three tables as migrations; sqlty reads the same files |
| 2 | `02-inference.sh` | 5:00 | A commit: the hook writes row and parameter types; `Decimal`, `count`, `LEFT JOIN` |
| 3 | `03-rows.sh` | 8:00 | Dataclasses with three mistakes; the quick fix; `--explain` cites the manual |
| 4 | `04-api.sh` | 12:00 | The request body is `BookParams`; ty catches a wrong parameter name |
| 5 | `05-migration.sh` | 15:00 | A nullable column: the hook skips SQL-only commits; then sqlty, then ty |

## The hooks

```toml
[[repos]]
repo = "https://github.com/jirikuncar/sqlty"
rev = "v0.3.0"
hooks = [{ id = "sqlty-rewrite" }, { id = "sqlty-check", always_run = true, pass_filenames = false }]
```

The published hooks run on staged Python files. A migration changes the types of
code it doesn't touch, so step 5 makes `sqlty-check` run on every commit and check
the whole project (`sqlty check` with no path checks `.`).
