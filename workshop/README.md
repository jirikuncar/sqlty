# DuckDesk: shipping on a Friday with a rubber duck

A 20-minute live-coding talk. We build DuckDesk, a startup that rents rubber
ducks to developers by the hour, and deploy it on every push, from a laptop.
sqlty types the SQL, Logfire traces it, prek is the whole pipeline (commit,
merge and push), the editor fixes what a merge broke, and Docker runs two
versions side by side while a migration rolls out.

[TRANSCRIPT.md](TRANSCRIPT.md) is what to say while typing.
[recording/](recording/) makes a narrated video of the same story.

## Before the talk

- [uv](https://docs.astral.sh/uv/), git, Docker with Compose
- VS Code with the [sqlty extension](https://marketplace.visualstudio.com/items?itemName=jirikuncar.sqlty),
  with the app's `.venv` selected as the Python interpreter
- Optional: a Logfire project (`logfire auth`), and `LOGFIRE_TOKEN` exported for the
  containers

Pull `postgres:17-alpine`, `debian:trixie-slim` and `ghcr.io/astral-sh/uv:0.11`
before going on stage: conference Wi-Fi is not a deployment platform.

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
| 0 | `00-setup.sh` | 0:45 | `uv init`, dependencies |
| 1 | `01-migrations.sh` | 1:30 | Postgres in Compose; three tables as migrations; a small migrator |
| 2 | `02-queries.sh` | 3:00 | `sqlty rewrite` writes row and parameter types, `INSERT`/`UPDATE ... RETURNING` included |
| 3 | `03-api.sh` | 6:00 | The request body is the query's parameters; `bind`, ty, tests |
| 4 | `04-logfire.sh` | 7:30 | Request and query spans, tagged with the version |
| 5 | `05-prek.sh` | 8:30 | Tests pass, prek still rejects the commit |
| 6 | `06-deploy.sh` | 10:30 | `git push` builds an image, migrates and rolls it out |
| 7 | `07-bobs-merge.sh` | 12:00 | Two green branches; prek blocks the merge; the quick fix |
| 8 | `08-rolling-deploy.sh` | 15:30 | `sqlty breaking` stops the deploy; expand, then contract |

## Names

| Say | Not |
|-----|-----|
| sqlty: "sequel-tee" | "squilty" |
| uv: "you-vee", ty: "tee-why", prek: "preck" | |
| colour: "colour, with a U"; color: "color, without the U" | the same word twice |

## Recording

`recording/make-video.sh` plays the talk in tmux (Neovim with `sqlty server` on the
left, a shell and the server on the right), types slowly, records it with asciinema
and renders `build/duckdesk.mp4` with narration and subtitles. It runs the real
commands, so it needs what the talk needs, plus:

- [Neovim](https://neovim.io) 0.11, tmux, ffmpeg
- [asciinema](https://asciinema.org) 3 and [agg](https://github.com/asciinema/agg)
- [Piper](https://github.com/OHF-Voice/piper1-gpl) (`uv tool install piper-tts`) and a voice,
  such as `en_US-lessac-medium.onnx`

```bash
cd recording
export PIPER_MODEL=~/voices/en_US-lessac-medium.onnx
./make-video.sh                          # every chapter
./make-video.sh --chapters merge         # after a review: record one chapter again
```

Each scene of `screenplay.py` is a chapter, recorded and rendered on its own to
`build/chapters/<name>.mp4`; `build/duckdesk.mp4` joins them, with a chapter list.
Recording a chapter plays the chapters before it fast and unrecorded, so the app
is where the chapter starts, and only chapters whose recording changed render again.

The narration is in `screenplay.py`; the code it types comes from the step
scripts. To use your own voice, record the lines that `build/cue-sheet.md`
lists as `voice/own/<id>.wav` and run `make-video.sh` again: the video waits
for each of your takes, however long it is. Idle time (installs, builds) is
cut to 1.5 seconds, never while the voice speaks.
