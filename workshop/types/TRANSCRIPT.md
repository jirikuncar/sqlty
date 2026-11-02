# Transcript

Lines in quotes are said aloud. Code blocks are typed or run. Each step's
script has the full code: keep it open on a second screen. One terminal, one
editor with the sqlty extension (and ty's, if you have it).

## Cold open (0:00)

"Every developer has a rubber duck. You explain your bug to the duck. The duck says
nothing. And halfway through your sentence, you find the bug.

This is DuckDesk: rubber ducks, rented by the hour. Last week, a customer asked for a
duck badge, and got a 500. The duck said nothing. The logs said: `NoneType` has no
attribute `upper`.

Today we don't deploy anything. Today, the types talk back. The database knows what
every column holds. Python should know it too, before a customer finds out."

## Step 0: an empty project, with hooks (1:00)

Run `00-setup.sh`, or type:

```bash
uv init --bare --app --name duckdesk --python 3.14
uv add fastapi uvicorn "psycopg[binary]" "sqlty>=0.3.0"
uv add --dev pytest httpx2 prek ty
```

Paste `prek.toml`.

"Three tools check this project. sqlty checks the SQL against the schema. ty checks
the Python. ruff formats it. prek runs them on every commit.

The sqlty and ruff hooks are the published ones: a repository and a version. I
didn't write them. prek builds each hook once and caches it. The sqlty hook
compiles Rust, so I built it before the talk. That's what `prepare-hooks` does."

```bash
uv run prek install
uv run prek prepare-hooks
```

## Step 1: the schema (3:00)

```bash
docker compose up -d --wait db
```

Show `migrations/001_tables.sql`.

"Three tables. `ducks`: a name, a colour, and a price per hour. `developers` are our
customers. `consultations` say which duck heard which bug, and if the bug is solved."

"Our ducks: Sir Quack costs 99 an hour. Admiral Bubbles costs 42. Segfault Sam is free.
He crashes a lot."

```toml
[tool.sqlty]
schema = "migrations/"
param-types = true
```

"sqlty reads the same migrations as the database. There is no second copy of the
schema to keep in sync. And `param-types`: sqlty types what goes into a query, not
only what comes out."

## Step 2: SQL in, types out (5:00)

Type `queries.py` as plain strings. Start each with `--sql` and a newline.

"Four queries, as plain strings. `--sql` marks them as SQL. List the ducks. Book a
consultation, an `INSERT` that returns the new row. Solve one, an `UPDATE` that
returns it. And a leaderboard: each duck, the bugs it solved, and the hardest one."

```bash
git add -A
git commit -m "Queries"
```

"The commit stops. The sqlty-rewrite hook changed my file, and ruff formatted it.
Let's see what they wrote."

```bash
git diff
```

"Each query is now `SQL` of two types: the row, and the parameters. The price is
`NUMERIC`, so Python gets a `Decimal`. `count` is an `int`: it counts rows, and
zero rows count as zero, never `NULL`. And the hardest bug is `str` or `None`. Why?
The `LEFT JOIN`. A duck that solved nothing still gets a row, with `NULL`s on the
right side. `max` of nothing is `NULL`.

And for the `INSERT`, sqlty wrote `BookParams`: a `TypedDict` with the three values
the query needs. I wrote none of these types. The schema did."

```bash
git commit -m "Queries, typed by the schema"
```

## Step 3: rows with names (8:00)

"Tuples are fine, but names are nicer." Write the dataclasses and replace each
`tuple[...]` with its row type. Make three mistakes on purpose:
`hourly_rate: float`, `hardest_bug: str`, and a new query with a typo, `colur`.

"Red lines. The editor runs the same sqlty as the hook. Three mistakes.

One: `colur`. There is no such column. Did you mean colour? Quick fix.

Two: money is a float. Sir Quack costs 99 an hour, not 98.99999999. The column is
`NUMERIC`, and sqlty says: that's a `Decimal`.

Three: the hardest bug is `str`. But we just saw: it can be `NULL`. Why does sqlty
think so? Ask it."

```bash
uv run sqlty check --explain .
```

"Every type comes from a rule, and every rule cites the PostgreSQL manual. `count`:
gives zero, not `NULL`, over no rows. `max`: keeps the type of its argument. The
argument comes from the right side of a `LEFT JOIN`, so it can be `NULL`."

Fix the two annotations: `decimal.Decimal` and `str | None`.

```bash
uv run sqlty check .
git commit -am "Rows with names"
```

## Step 4: the API (12:00)

Paste `main.py`. Point at `book`.

"The request body of `book` is `BookParams`, the parameters of the `INSERT`. FastAPI
validates it and puts it in the API docs. So the API contract comes from the SQL."

Point at `solve`, with `{"consultation_id": consultation_id}`.

"A red line again, but from ty this time. The query names its parameter `id`, not
`consultation_id`. ty knows, because sqlty wrote `SolveParams`. Without it, this is a
`KeyError` at run time. Or worse, a query that runs with the wrong value."

```bash
uv run ty check
```

Fix it to `{"id": consultation_id}`. Paste `conftest.py` and `test_main.py`.

"Badges are shouted: ducks don't whisper. And the test for the hardest bug: `None`
until a bug is solved, then the bug."

```bash
uv run pytest -q
git add -A
git commit -m "The API"
```

## Step 5: Captain Clear (15:00)

"Our designer made a new duck. It's made of glass. It has no colour. Its name is
Captain Clear."

Type `migrations/003_captain_clear.sql`.

```sql
ALTER TABLE ducks ALTER COLUMN colour DROP NOT NULL;

INSERT INTO ducks (name, colour, hourly_rate) VALUES ('Captain Clear', NULL, 7.00);
```

```bash
uv run python migrate.py
uv run pytest -q
git add -A
git commit -m "Captain Clear"
```

"Tests pass. The commit goes through. Ship it? Look at the hooks: skipped. The
published sqlty hook runs on Python files, and I changed only SQL. Remember this:
a migration changes the types of Python code it doesn't touch.

One line in `prek.toml`: check the whole project on every commit."

```toml
{ id = "sqlty-check", always_run = true, pass_filenames = false }
```

```bash
git commit -am "Check every commit"
```

"Now sqlty sees it: `Duck.colour` is `str`, but the column can be `NULL`. Fair. I
make it `str | None`."

```bash
git commit -am "Check every commit"
```

"Now sqlty is happy, and ty is not. The badge calls `.upper()` on the colour. On
Captain Clear, that's `None.upper()`. That's the 500 from last week. No test asks for
his badge, so the first customer who asks finds it, in production.

The schema changed in SQL. sqlty carried the change into the Python types. ty
followed it to the one line that breaks. Neither tool alone finds this bug."

Fix it with `(duck.colour or 'invisible')` and add the test.

```bash
uv run pytest -q
git commit -am "Captain Clear is invisible"
```

## Wrap-up (19:00)

"What did we write by hand? SQL, and the names of our rows. sqlty wrote the row
types, the parameter types, and their imports, from the same migrations the database
runs. When a type was wrong, it said why, and cited the manual. ty took the types
from there into every line of Python. And two published hooks, plus one line of
config, made sure no commit skips any of it.

Captain Clear is invisible. His badge says so. Questions?"
