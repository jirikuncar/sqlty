# Transcript

Lines in quotes are said aloud. Code blocks are typed or run. Each step's
script has the full code: keep it open on a second screen. Two terminals: one
for typing, one for `./traffic.sh` from step 6 on.

`colour` and `color` sound the same. Say "colour, with a U" and "color, without
the U", and point at the screen.

## Cold open (0:00)

"Every developer has a rubber duck. You explain your bug to the duck. The duck says
nothing. And halfway through your sentence, you find the bug.

Ducks are the best debugging tool ever made. But nobody sells them as a service.
Until today. This is DuckDesk: rubber ducks, rented by the hour.

We meet investors in 20 minutes. They want a product that works, runs in production,
and ships on every push. We have no money for a CI server, so everything runs on
this laptop. And it's Friday. Developers say: never deploy on a Friday. Let's go."

## Step 0: an empty project (0:45)

Run `00-setup.sh`, or type:

```bash
uv init --bare --app --name duckdesk --python 3.14
uv add fastapi uvicorn "psycopg[binary]" "logfire[fastapi,psycopg]" "sqlty>=0.3.0"
uv add --dev pytest httpx2 prek ruff ty
```

"uv gives us Python and a lockfile. The app is FastAPI and Postgres. sqlty checks our
SQL, ty checks our Python, and prek runs git hooks. By the end, prek is our whole
pipeline."

## Step 1: migrations (1:30)

```bash
docker compose up -d --wait db
```

Type `migrations/001_tables.sql`.

"Three tables. `ducks`: a name, a colour, and a price per hour. Colour, with a U. That
is British spelling, because our best duck is British: Sir Quack. `developers` are our
customers. `consultations` say which duck heard which bug, and if the bug is solved."

"Our ducks: Sir Quack costs 99 an hour. Admiral Bubbles costs 42. Segfault Sam is free.
He crashes a lot."

Paste `migrate.py`.

"A small migrator: each file runs once, in order, in a transaction. One rule: when a
migration has run in production, we never change it. Remember this rule. We need it
later."

```toml
[tool.sqlty]
schema = "migrations/"
```

"sqlty reads the migrations folder as the schema. It never connects to the database.
It reads the SQL files, `ALTER TABLE` too, and knows what the tables look like now."

## Step 2: typed queries (3:00)

Add `param-types = true`, then type `queries.py` with plain strings.

"A query is a normal string. The first line is `--sql`. For the database, that line is
a comment. For sqlty, it marks a query. Booking a duck is an `INSERT`. Solving a bug is
an `UPDATE`. Both use `RETURNING`, so they give back the row."

```bash
uv run sqlty rewrite .
```

Pause on the result.

"I did not write these types. sqlty read the migrations. `hourly_rate` is `NUMERIC NOT
NULL`, so it is a `Decimal`, never `None`. Money in a float is how startups lose money."

"The second type is the parameters. `BookParams` was generated: `duck_id` is `int`, not
`None`, because the column is `NOT NULL`. And it is still a normal string, so psycopg
takes it."

Run `./steps/run.sh 2` to switch the tuples to dataclasses, then show the file.

"Names are nicer than tuples. sqlty keeps my dataclasses and checks every field against
its column: name, type, and `None` or not."

## Step 3: the API (6:00)

Paste `main.py`. Talk only about `book`.

"The request body is `BookParams`: the parameters of the query. FastAPI checks it and
puts it in the API docs. So the API contract comes from the SQL. `bind` checks the
parameters, and ty checks `bind`."

```bash
uv run pytest -q
```

"Each test run starts with a new, empty database. The test books Admiral Bubbles for
'works on my machine' and marks it solved. In real life, 'works on my machine' is never
solved. In tests, everything is."

## Step 4: Logfire (7:30)

```python
logfire.configure(service_name="duckdesk", service_version=VERSION, send_to_logfire="if-token-present")
logfire.instrument_psycopg()
logfire.instrument_fastapi(app)
```

"Three lines, and we have OpenTelemetry traces. Without a token, nothing leaves this
laptop. Remember `service_version`: we need it later."

Show the Logfire live view, with requests already running.

"Here is the request, the SQL inside it, and our log. Sir Quack replies very fast. He
never says anything."

## Step 5: prek (8:30)

```bash
uv run prek install
```

"prek runs ruff, sqlty, ty and the tests. It runs before every commit, and before every
merge commit. Remember the merge."

Type the `/backlog` endpoint, with `open_bugs: str`. Commit.

"Investors love a backlog. I'm in a hurry, so the type says `str`."

The commit fails at `sqlty check`. Point at `pytest....Passed`.

"The tests pass. No test calls `/backlog`, so the first real user finds out, in
production. ty can't see it, because the SQL is a string. sqlty reads the string."

Fix it to `int`, commit.

## Step 6: push to deploy (10:30)

Show three parts of `deploy.sh`: build, migrate, start next to the old version.

"This is our deploy server. It builds the image from the commit. It runs the
migrations. It starts the new version next to the old one. Both versions run for a few
seconds. Then we stop the old one, and tag the commit `prod`."

"prek runs it before `git push`. So `git push` is our deploy button. When the deploy
fails, the push fails."

```bash
git push -u origin main
./traffic.sh        # second terminal
```

"Version one is live. Now, let me introduce Bob."

## Step 7: Bob's merge (12:00)

"Bob is our second developer. Bob reads the style guide. The style guide says: American
English. Bob follows every rule."

Create `feature/ducks-by-colour`: the query, the endpoint, a test. Commit. Green.

"On his branch, Bob writes migration 003. He drops the U: colour becomes color. He
updates his queries too. His branch is green. Mine is green. Merge Bob's, then mine."

The second merge stops at `sqlty check` and pytest.

"Git merges text, not meaning. My new query reads colour, with a U. That column is gone
on Bob's branch. Neither branch was wrong. Only the two together. The merge hook
catches it, and the merge stays open."

Open `queries.py` in the editor.

"The editor runs the same sqlty as the hook. The red line is under colour: did you mean
color, without the U? One click on the quick fix."

`git commit` finishes the merge.

## Step 8: the rolling deploy (15:30)

"Everything is green. Ship it? Look at the traffic. Version one still runs, and it reads
colour, with a U. The migration runs first. It renames the column. And version one
fails on every request, until the new version is up. Every check looked at one version
of the code. A deploy runs two versions at once."

Add the check to `deploy.sh`:

```bash
if git rev-parse -q --verify prod >/dev/null; then
  uv run sqlty breaking --against prod --head "$ref" --direction backward
fi
```

"`sqlty breaking` checks the code in production against the new schema."

```bash
git push
```

"Refused: production reads colour. Migration 003 never ran in production. Only in test
databases that we throw away. So we can still change it. We add the new column next to
the old one, and copy the data."

```sql
ALTER TABLE ducks ADD COLUMN color TEXT NOT NULL DEFAULT '';
UPDATE ducks SET color = colour;
```

```bash
git push
```

Point at `traffic.sh`.

"Two versions, one database, both fine. The old one says colour, the new one says
color. Logfire tells them apart by `service_version`. Nobody adds ducks during the
deploy, so one copy of the data is enough. Next week, migration 004 drops colour."

## Wrap-up (19:00)

"In 20 minutes: types from migrations, traces in three lines, a hook that caught what
the tests missed, a merge that stopped, an editor that fixed it, and a deploy that kept
production alive. We used no CI server. Sir Quack lost his U. He is still not talking to
Bob. DuckDesk is now open for investment. We accept ducks."
