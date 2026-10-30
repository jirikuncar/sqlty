# Transcript

Lines in quotes are said aloud. Code blocks are typed or run. Each step's
script has the full code: keep it open on a second screen.

## Cold open (0:00)

"Every developer has a rubber duck. You explain the bug to the duck, the duck
says nothing, and halfway through your own sentence you find the bug. Ducks are
the most effective debugging tool ever made, and nobody is selling them as a
service. Until today.

This is DuckDesk: rubber ducks, rented by the hour. We pitch to investors in 30
minutes. They want a working product, deployed, with observability, and a
pipeline that ships on every push. On a Friday. Let's go."

## Step 0: an empty project (0:30)

```bash
mkdir duckdesk && cd duckdesk && git init -b main
uv init --bare --app --name duckdesk --python 3.14
uv add fastapi uvicorn "logfire[fastapi,sqlite3]" "sqlty>=0.3.0"
uv add --dev pytest httpx2 prek ruff
```

"uv gives us Python 3.14 and a lockfile, so CI and the server get exactly this.
FastAPI for the API, Logfire for telemetry, sqlty for the SQL. The dev group is
pytest and prek, the git hook runner, which comes back later in the story."

## Step 1: the schema (2:00)

Type `schema.sql`. Read the table names aloud as you go.

"Three tables. `ducks`: every duck has a name, a colour, and an hourly rate.
Note the British spelling of colour: our premium duck, Sir Quacksalot, insists on it.
`developers`: our customers. `consultations`: who asked which duck about which bug,
and whether the duck solved it."

"Our inventory: Sir Quacksalot, 99 an hour. Admiral Squeaks, 42. And Segfault Sam,
who is free. You get what you pay for."

```toml
[tool.sqlty]
schema = "schema.sql"
dialect = "sqlite"
```

"These three lines tell sqlty where the schema is. It never connects to a database:
it reads the DDL."

## Step 2: typed queries (4:00)

Type `queries.py` with plain strings first.

"The queries are plain strings starting with `--sql`, a newline, then the SQL. To Python
that's a string. To the database it's a comment. To sqlty it's a query."

```bash
uv run sqlty rewrite .
```

Pause on the diff.

"I never wrote `tuple[int, str, str, float]`. sqlty read the schema: `id` is an
`INTEGER PRIMARY KEY`, `hourly_rate` is `REAL NOT NULL`, so it's a float and can't be
`None`. Even `count(c.id)` through a `LEFT JOIN` came out as `int`. And `SQL[T]` is
a `str` subclass, so `sqlite3` takes it as is. It costs nothing at runtime."

Replace the tuples with the `Duck` and `Champion` dataclasses.

"Tuples are fine, but names are nicer. I swap in dataclasses, and now sqlty checks
that the fields match the columns: names, types and nullability."

```bash
uv run sqlty check .
```

## Step 3: the API (9:00)

Type `main.py`. Spend the time on `fetch_all`.

"`row_form` gives back the `T` of `SQL[T]`, here the `Duck` class, so one five-line
helper turns rows into dataclasses, and the return type is `list[Duck]` without a cast."

```bash
uv run pytest -q
uv run uvicorn main:app --reload
curl localhost:8000/ducks
```

"The investors' first question: which duck is best? `/leaderboard`. The test books
Admiral Squeaks for 'works on my machine', marks it solved, and the Admiral is
crowned. Very realistic: 'works on my machine' is never really solved."

## Step 4: Logfire (12:00)

```python
logfire.configure(service_name="duckdesk", send_to_logfire="if-token-present")
logfire.instrument_sqlite3()
logfire.instrument_fastapi(app)
```

"Three lines, and this is OpenTelemetry under the hood. Instrument `sqlite3` before
the first `connect`, because it patches `connect`. `if-token-present` keeps tests and
laptops quiet: without a token, spans go to the console only."

```bash
logfire auth && logfire projects use duckdesk
curl -X POST localhost:8000/consultations -H 'content-type: application/json' \
  -d '{"duck_id": 1, "developer_id": 2, "bug": "it is always DNS"}'
```

Switch to the Logfire live view.

"Here's the request, the SQL inside it, the query text right there, and our log:
'duck 1 hears about it is always DNS'. When the investors ask for metrics, this
is them. Sir Quacksalot's response time is excellent: the duck never answers."

## Step 5: prek (15:00)

```bash
uv run prek install
```

"prek is a faster drop-in for pre-commit, written in Rust. It reads `prek.toml`:
ruff, `sqlty rewrite`, `sqlty check`, and pytest. The same file runs in CI, so 'works on
my machine' finally means 'works in CI'."

Type the `/backlog` endpoint, with `open_bugs: str`. Commit.

"Investors love a backlog. I'm in a hurry, so the type says `str`..."

The commit fails at `sqlty check`. Point at `pytest....Passed`.

"Look: the tests passed. A dataclass doesn't validate, so `open_bugs` is quietly an
`int` in a field that says `str`, and the first caller who does `.upper()` on it finds out
in production. The tests didn't catch it. The type checker didn't, because the SQL is a
string. sqlty did, because it reads the SQL."

Fix it to `int`, commit, all green.

## Step 6: CI/CD (19:00)

Show `.github/workflows/ci.yml`, already pushed before the talk.

"Two jobs. `check` runs the exact same `prek run --all-files`. `deploy` needs `check`
and runs only on `main`: `flyctl deploy` builds the container on Fly's builders.
`concurrency: deploy` means two merges never deploy at once. Logfire's token is a Fly
secret, so production traces land in the same project."

```bash
git tag prod
git push --tags origin main
```

"The tag marks what runs in production; it comes back at the end."

"While that ships, let me introduce Bob."

## Step 7: Bob's merge (22:00)

"Bob is our second engineer. Bob read the style guide. The style guide says American
English. Bob is very thorough."

Create `feature/ducks-by-colour`: the `DUCKS_BY_COLOUR` query, the `/ducks/{colour}`
endpoint, a test. Open a pull request. Green.

"Meanwhile, on Bob's branch:"

```bash
sed -i 's/colour/color/g' schema.sql seed.sql queries.py
```

"Bob renamed the column, his query and the dataclass field. His pull request: green.
Mine: green. GitHub says 'no conflicts'. Merge, merge."

```bash
uv run sqlty check .
```

"Red. Git merges text, not meaning. My new query reads `colour`, a column that
stopped existing five minutes ago. Neither branch was wrong; only the two together.
CI on `main` goes red, and `deploy` needs `check`, so the broken build never reaches
production. Sir Quacksalot is safe."

Open `queries.py` in VS Code.

"The extension runs `sqlty server`, the same sqlty that CI runs, with the same
config. The squiggle is under `colour`: 'did you mean `color`?' Quick Fix, 'Fix
unknown-column'. Or save: the workspace runs `source.fixAll.sqlty` on save."

Show that the parameter `:colour` stayed: it's ours, not the table's. Commit, push,
watch the deploy go green.

## Step 8: the rolling deploy (27:00)

"One more thing. Fly replaces machines one at a time. For a few seconds the old
code and the new schema run side by side."

```bash
uv run sqlty breaking --against prod
```

"DuckDesk keeps its data in memory, but put it in Postgres and this is what you get.
Backward: the code in production asks for `colour`, which the new schema doesn't have.
Forward: the new code asks for `color`, which the old schema doesn't have. CI was green
and the deploy would still break. So `sqlty breaking` goes into CI on every pull request,
against the base branch. The cure is the boring one: add the new column, move the
code, drop the old column, in three deploys."

## Wrap-up (29:00)

"In 30 minutes: types from the schema instead of by hand, traces from three lines,
a hook that caught what the tests missed, an editor that fixed what a merge broke,
and a pipeline that wouldn't deploy any of it broken. The duck said nothing the whole
time, which is exactly why it works. DuckDesk is now accepting investment, in ducks."
