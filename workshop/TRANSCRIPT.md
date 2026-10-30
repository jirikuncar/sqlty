# Transcript

Lines in quotes are said aloud. Code blocks are typed or run. Each step's
script has the full code: keep it open on a second screen. Two terminals: one
for typing, one for `./traffic.sh` from step 6 on.

## Cold open (0:00)

"Every developer has a rubber duck. You explain the bug to the duck, the duck
says nothing, and halfway through your own sentence you find the bug. Ducks are
the most effective debugging tool ever made, and nobody is selling them as a
service. Until today.

This is DuckDesk: rubber ducks, rented by the hour. We pitch to investors in 30
minutes. They want a working product, deployed, observable, and shipping on every
push. Our budget for CI is zero, so the pipeline runs on this laptop. On a Friday.
Let's go."

## Step 0: an empty project (0:30)

```bash
mkdir duckdesk && cd duckdesk && git init -b main
uv init --bare --app --name duckdesk --python 3.14
uv add fastapi uvicorn "psycopg[binary]" "logfire[fastapi,psycopg]" "sqlty>=0.3.0"
uv add --dev pytest httpx2 prek ruff ty
```

"uv gives us Python 3.14 and a lockfile, so the container gets exactly this. FastAPI
and psycopg for the app, Logfire for telemetry, sqlty for the SQL, ty to type check.
prek is a git hook runner, and by the end of the talk it is our entire CI/CD."

## Step 1: migrations (2:00)

```bash
docker compose up -d --wait db
```

Type `migrations/001_tables.sql`. Read the table names aloud as you go.

"Three tables. `ducks`: every duck has a name, a colour, and an hourly rate.
Note the British spelling of colour: our premium duck, Sir Quacksalot, insists on it.
`developers`: our customers. `consultations`: who asked which duck about which bug,
and whether the duck solved it."

"`002_ducks.sql` is our inventory: Sir Quacksalot, 99 an hour. Admiral Squeaks, 42.
And Segfault Sam, who is free. You get what you pay for."

Paste `migrate.py`.

"Twenty lines of migrator: each file once, in order, inside a transaction, and an
advisory lock so two deploys never migrate at once. The rule: a migration that has
run anywhere is never edited again. Write that down; it comes back."

```toml
[tool.sqlty]
schema = "migrations/"
```

"And this is the trick: sqlty reads the migrations folder as the schema. It never
connects to a database. It replays the DDL, `ALTER TABLE`s included, and knows what
the tables look like after the last file."

## Step 2: typed queries (5:00)

Add `param-types = true` under `[tool.sqlty]`, then type `queries.py` with plain strings.

"The queries are plain strings starting with `--sql`, a newline, then the SQL. To Python
that's a string. To Postgres it's a comment. To sqlty it's a query. Booking is an
`INSERT ... RETURNING`, solving a bug an `UPDATE ... RETURNING`: both hand back the row
they wrote."

```bash
uv run sqlty rewrite .
```

Pause on the result.

"I never wrote `tuple[int, str, str, Decimal]`. sqlty read the migrations: `hourly_rate`
is `NUMERIC NOT NULL`, so it's a `Decimal`, never `None`. Money in a float is how
startups lose money. Even `count(c.id)` through a `LEFT JOIN` came out as `int`."

"And the second type argument is the parameters. `BookParams` is a `TypedDict` sqlty
generated: `duck_id: int`, not `int | None`, because the column it is written to is
`NOT NULL`. `SolveParams.id` is `int | None`: a `WHERE` comparison takes a NULL, it just
matches nothing. `SQL[R, P]` is a function from `P` to `R`, and it's still a `str`
subclass: psycopg takes it as is."

Replace the row tuples with the `Duck`, `Consultation` and `Champion` dataclasses, write
`SQL[Duck](...)` without parameters, and rewrite again.

"Names are nicer than tuples. `rewrite` keeps the row types I wrote and fills in the
parameters, and `sqlty check` makes sure the fields match the columns: names, types
and nullability. One `Consultation` serves the `INSERT` and the `UPDATE`."

## Step 3: the API (9:00)

Type `main.py`. Spend the time on `book` and `solve`.

"`execute` from `sqlty.drivers.psycopg` reads the `R` of `SQL[R, P]` and picks psycopg's
row factory: `Duck` objects come out, and the type checker sees `list[Duck]`."

"The body of `POST /consultations` is `BookParams`, the parameters of the query. FastAPI
validates it and writes it into the OpenAPI docs, so the API contract comes from the
SQL. `bind` checks the parameters against `P`: pass `{"id": "seven"}` and ty rejects it
before anything runs. `solve` returns the row the `UPDATE` wrote, or `None` when no row
matched, which is our 404."

```bash
uv run pytest -q
uv run ty check
```

"`conftest.py` drops a test database and runs the same migrations on it. The test books
Admiral Squeaks for 'works on my machine', marks it solved, and the Admiral is crowned.
Very realistic: 'works on my machine' is never really solved."

## Step 4: Logfire (12:00)

```python
logfire.configure(service_name="duckdesk", service_version=VERSION, send_to_logfire="if-token-present")
logfire.instrument_psycopg()
logfire.instrument_fastapi(app)
```

"Three lines of OpenTelemetry. `service_version` will matter in ten minutes, when two
versions run at once. Without a token, spans go to the console only, so tests stay quiet."

```bash
uv run uvicorn main:app --reload
curl -X POST localhost:8000/consultations -H 'content-type: application/json' \
  -d '{"duck_id": 1, "developer_id": 2, "bug": "it is always DNS"}'
```

Switch to the Logfire live view.

"The request, the SQL inside it, and our log: 'duck 1 hears about it is always DNS'.
Sir Quacksalot's response time is excellent: the duck never answers."

## Step 5: prek (14:00)

```bash
uv run prek install
```

"`prek.toml`: ruff, `sqlty rewrite`, `sqlty check`, ty, pytest. Installed for two
moments: before a commit, and before a merge commit. Remember the second one."

Type the `/backlog` endpoint, with `open_bugs: str`. Commit.

"Investors love a backlog. I'm in a hurry, so the type says `str`..."

The commit fails at `sqlty check`. Point at `pytest....Passed`.

"The tests passed, and so did ty. A dataclass doesn't validate, so `open_bugs` is quietly
an `int` in a field that says `str`, and the first caller who does `.upper()` finds out in
production. ty can't see it, because the SQL is a string. sqlty reads the string."

Fix it to `int`, commit, all green.

## Step 6: push to deploy (17:00)

Show `deploy.sh`.

"Our CD server. `git archive` builds the image from the commit, not my working tree, so
what was checked is what ships. Run the migrations. Start the new version next to the old
one. Wait until it answers. Keep both for a few seconds, the way a fleet rolls over, then
retire the old one and tag the commit `prod`."

"prek runs it as a `pre-push` hook. `git push` is the deploy button. If the deploy fails,
the push fails, and `main` on the remote never points at something that isn't running."

```bash
git push -u origin main
./traffic.sh        # second terminal
```

"Version one is live. While it serves ducks, let me introduce Bob."

## Step 7: Bob's merge (21:00)

"Bob is our second engineer. Bob read the style guide. The style guide says American
English. Bob is very thorough."

Create `feature/ducks-by-colour`: the `DUCKS_BY_COLOUR` query, the `/ducks/{colour}`
endpoint, a test. Save: the fix on save adds `DucksByColourParams`. Commit. Green.

"Meanwhile, on Bob's branch: migration 003."

```sql
ALTER TABLE ducks RENAME COLUMN colour TO color;
```

"Bob renamed the column, his query and the dataclass field. Green. Mine: green. Git
says no conflicts. Merge Bob's, then mine."

The second merge stops at `sqlty check` and pytest.

"Git merges text, not meaning. My new query reads `colour`, a column that stopped existing
on Bob's branch. Neither branch was wrong; only the two together. Without a CI server,
the `pre-merge-commit` hook is what notices, and the merge stays open, like a conflict."

Open `queries.py` in VS Code.

"The extension runs `sqlty server`: same sqlty, same config as the hook. The squiggle is
under `colour`: 'did you mean `color`?' Quick Fix, 'Fix unknown-column', or just save."

Show that the parameter `%(colour)s` stayed: it's ours, not the table's. `git commit`
finishes the merge.

## Step 8: the rolling deploy (25:00)

"Everything is green. Ship it? Look at `traffic.sh`: version one is still serving, and
it reads `colour`. Migrations run first. The moment 003 renames the column, version one
starts throwing 500s, and it keeps serving until the new version is up. Every check we
have looked at one revision. A deploy is two."

Add the guard to `deploy.sh`:

```bash
if git rev-parse -q --verify prod >/dev/null; then
  uv run sqlty breaking --against prod --head "$ref" --direction backward
fi
```

"`sqlty breaking`: the queries of `prod` against the schema of what we're pushing.
Backward only, because our deploy migrates first."

```bash
git push
```

"Refused: `prod` reads `colour`. So we do the boring thing. Migration 003 has never run
anywhere but a test database, so we may still change it: expand instead of rename."

```sql
ALTER TABLE ducks ADD COLUMN color TEXT NOT NULL DEFAULT '';
UPDATE ducks SET color = colour;
```

```bash
git push
```

Point at `traffic.sh`.

"Two versions, one database, both 200. The old one says `colour`, the new one says
`color`. Logfire tells them apart by `service_version`. Then the old one retires."

"Contract: migration 004 drops `colour`. `sqlty breaking` checks it against the new `prod`,
which no longer reads it."

```bash
git push
```

## Wrap-up (29:00)

"In 30 minutes: types from the migrations instead of by hand, traces from three lines,
a hook that caught what the tests missed, a merge that refused to complete, an editor
that fixed it, and a deploy that refused to break the version still running. No CI server
was harmed. The duck said nothing the whole time, which is exactly why it works.
DuckDesk is now accepting investment, in ducks."
