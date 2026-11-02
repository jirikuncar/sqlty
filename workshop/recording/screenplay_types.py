"""The recorded types talk (../types/README.md): narration, typing and commands, scene by scene."""

from pathlib import Path

import kit
from kit import Fix, Hold, Keys, Open, Run, Say, Scene, Write, number

STEPS = Path(__file__).parent.parent / "types" / "steps"


def code(step: str, path: str, nth: int = 0) -> str:
    return kit.code(step, path, nth, steps=STEPS)


def commit(message: str, ok: bool | None = True) -> Run:
    return Run(f"git add -A && git commit -qm '{message}'", ok=ok)


README = """# DuckDesk

Rubber ducks, rented by the hour.

- Talks to every developer
- Never interrupts
- Never returns a 500
"""

SCENES = number(
    [
        Scene(
            "open",
            [
                Write("README.md", README, typed=False),
                Say(
                    "Every developer has a rubber duck. You explain your bug to the duck. The duck says nothing. "
                    "And halfway through your sentence, you find the bug."
                ),
                Say(
                    "This is DuckDesk: rubber ducks, rented by the hour. Last week, a customer asked for a duck "
                    "badge, and got a 500. The duck said nothing. The logs said: NoneType has no attribute upper."
                ),
                Say(
                    "Today we don't deploy anything. Today, the types talk back. The database knows what every "
                    "column holds. Python should know it too, before a customer finds out."
                ),
            ],
        ),
        Scene(
            "setup",
            [
                Run("git init -q -b main"),
                Run("uv init -q --bare --app --name duckdesk --python 3.14"),
                Run('uv add -q fastapi uvicorn "psycopg[binary]" "sqlty>=0.3.0"'),
                Run("uv add -q --dev pytest httpx2 prek ty"),
                Write(".gitignore", ".venv/\n__pycache__/\n", typed=False),
                Write("pyproject.toml", code("00", "pyproject.toml"), typed=False, where="append"),
                Write("prek.toml", code("00", "prek.toml"), typed=False),
                Say(
                    "Three tools check this project. sqlty checks the SQL against the schema. ty checks the Python. "
                    "ruff formats it. prek runs them on every commit."
                ),
                Say(
                    "The sqlty and ruff hooks are the published ones: a repository and a version. I didn't write "
                    "them. prek builds each hook once and caches it. The sqlty hook compiles Rust, so I built it "
                    "before the talk. That's what prepare-hooks does."
                ),
                Run("uv run prek install"),
                Run("uv run prek prepare-hooks"),
                commit("An empty DuckDesk"),
            ],
        ),
        Scene(
            "schema",
            [
                Write("compose.yaml", code("01", "compose.yaml"), typed=False),
                Run("docker compose up -d --quiet-pull --wait db 2>/dev/null"),
                Write(
                    "migrations/001_tables.sql",
                    code("01", "migrations/001_tables.sql"),
                    typed=False,
                ),
                Say(
                    "Three tables. Ducks: a name, a colour, and a price per hour. Developers are our customers. "
                    "Consultations say which duck heard which bug, and if the bug is solved."
                ),
                Write(
                    "migrations/002_ducks.sql", code("01", "migrations/002_ducks.sql"), typed=False
                ),
                Say(
                    "Our ducks: Sir Quack costs 99 an hour. Admiral Bubbles costs 42. Segfault Sam is free. "
                    "He crashes a lot."
                ),
                Write("migrate.py", code("01", "migrate.py"), typed=False),
                Write("pyproject.toml", code("01", "pyproject.toml"), where="append"),
                Say(
                    "sqlty reads the same migrations as the database. There is no second copy of the schema to keep "
                    "in sync. And param-types: sqlty types what goes into a query, not only what comes out."
                ),
                Run("uv run python migrate.py"),
                commit("Ducks, developers, consultations"),
            ],
        ),
        Scene(
            "inference",
            [
                Say(
                    "Four queries, as plain strings. dash dash sequel marks them as SQL. List the ducks. Book a "
                    "consultation, an INSERT that returns the new row. Solve one, an UPDATE that returns it. "
                    "And a leaderboard: each duck, the bugs it solved, and the hardest one."
                ),
                Write("queries.py", code("02", "queries.py")),
                commit("Queries", ok=False),
                Say(
                    "The commit stops. The sqlty-rewrite hook changed my file, and ruff formatted it."
                ),
                Open("queries.py"),
                Say(
                    "Each query is now SQL of two types: the row, and the parameters. The price is NUMERIC, so "
                    "Python gets a Decimal. count is an int: zero rows count as zero, never NULL."
                ),
                Say(
                    "And the hardest bug is str or None. Why? The LEFT JOIN. A duck that solved nothing still gets "
                    "a row, with NULLs on the right side. max of nothing is NULL."
                ),
                Say(
                    "For the INSERT, sqlty wrote BookParams: the three values the query needs. I wrote none of "
                    "these types. The schema did."
                ),
                commit("Queries, typed by the schema"),
            ],
        ),
        Scene(
            "rows",
            [
                Say("Tuples are fine, but names are nicer. Dataclasses for the rows."),
                Write(
                    "queries.py",
                    "from dataclasses import dataclass",
                    where="before",
                    anchor="import decimal",
                ),
                Write(
                    "queries.py",
                    code("03", "rows.py"),
                    where="after",
                    anchor="from sqlty import SQL",
                ),
                Keys(
                    r":g/^LIST_DUCKS/s/tuple\[[^]]*\]/Duck/<CR>"
                    r":g/^BOOK\|^SOLVE/s/tuple\[[^]]*\]/Consultation/<CR>"
                    r":g/^LEADERBOARD/s/tuple\[[^]]*\]/Champion/<CR>:w<CR>"
                ),
                Say("And one more query: a single duck, for its badge."),
                Write("queries.py", code("03", "queries.py"), where="append"),
                Run("uv run sqlty rewrite -q ."),
                Hold(1),
                Say(
                    "Red lines. The editor runs the same sqlty as the hook. Three mistakes. One: colur. "
                    "There is no such column. Did you mean colour? Quick fix."
                ),
                Fix("queries.py"),
                Say(
                    "Two: money is a float. Sir Quack costs 99 an hour, not 98 point 9 9 9 9 9. The column is "
                    "NUMERIC, and sqlty says: that's a Decimal."
                ),
                Say(
                    "Three: the hardest bug is str. But we just saw: it can be NULL. Why does sqlty think so? Ask it."
                ),
                Run("uv run sqlty check --explain .", ok=False),
                Say(
                    "Every type comes from a rule, and every rule cites the PostgreSQL manual. count gives zero, "
                    "not NULL, over no rows. max keeps the type of its argument, and the LEFT JOIN makes it nullable."
                ),
                Keys(
                    r":%s/hourly_rate: float/hourly_rate: decimal.Decimal/<CR>"
                    r":%s/hardest_bug: str$/hardest_bug: str \| None/<CR>:w<CR>"
                ),
                Run("uv run sqlty check ."),
                Run("uv run prek run ruff-format --files queries.py >/dev/null || true"),
                commit("Rows with names"),
            ],
        ),
        Scene(
            "api",
            [
                Write("main.py", code("04", "main.py"), typed=False),
                Say(
                    "The request body of book is BookParams, the parameters of the INSERT. FastAPI validates it and "
                    "puts it in the API docs. So the API contract comes from the SQL."
                ),
                Keys('/consultation_id": <CR>'),
                Say(
                    "A red line again, but from ty this time. The query names its parameter id, not consultation "
                    "I D. ty knows, because sqlty wrote SolveParams. Without it, this is a KeyError at run time."
                ),
                Run("uv run ty check", ok=False),
                Keys(r':%s/"consultation_id": consultation_id/"id": consultation_id/<CR>:w<CR>'),
                Write("conftest.py", code("04", "conftest.py"), typed=False),
                Write("test_main.py", code("04", "test_main.py"), typed=False),
                Say(
                    "Badges are shouted: ducks don't whisper. And the hardest bug is None until a bug is solved, "
                    "then the bug."
                ),
                Run("uv run pytest -q"),
                commit("The API"),
            ],
        ),
        Scene(
            "migration",
            [
                Say(
                    "Our designer made a new duck. It's made of glass. It has no colour. Its name is Captain Clear."
                ),
                Write(
                    "migrations/003_captain_clear.sql",
                    code("05", "migrations/003_captain_clear.sql"),
                ),
                Run("uv run python migrate.py"),
                Run("uv run pytest -q"),
                commit("Captain Clear"),
                Say(
                    "Tests pass. The commit goes through. Ship it? Look at the hooks: skipped. The published sqlty "
                    "hook runs on Python files, and I changed only SQL. But a migration changes the types of "
                    "Python code it doesn't touch."
                ),
                Open("prek.toml"),
                Say("One line in prek.toml: check the whole project on every commit."),
                Keys(
                    ':%s/{ id = "sqlty-check" }/{ id = "sqlty-check", always_run = true, pass_filenames = false }/'
                    "<CR>:w<CR>"
                ),
                commit("Check every commit", ok=False),
                Open("queries.py"),
                Say(
                    "Now sqlty sees it: Duck.colour is str, but the column can be NULL. Fair. I make it str or None."
                ),
                Keys(r":%s/    colour: str$/    colour: str \| None/<CR>:w<CR>"),
                commit("Check every commit", ok=False),
                Open("main.py"),
                Say(
                    "Now sqlty is happy, and ty is not. The badge calls upper on the colour. On Captain Clear, "
                    "that's None dot upper. That's the 500 from last week. No test asks for his badge, so the "
                    "first customer who asks finds it, in production."
                ),
                Say(
                    "The schema changed in SQL. sqlty carried the change into the Python types. ty followed it to "
                    "the one line that breaks. Neither tool alone finds this bug."
                ),
                Keys(
                    r":%s/{duck.colour.upper()}/{(duck.colour or 'invisible').upper()}/<CR>:w<CR>"
                ),
                Write("test_main.py", code("05", "test_main.py"), where="append"),
                Run("uv run pytest -q"),
                commit("Captain Clear is invisible"),
            ],
        ),
        Scene(
            "wrap",
            [
                Say(
                    "What did we write by hand? SQL, and the names of our rows. sqlty wrote the row types, the "
                    "parameter types, and their imports, from the migrations the database runs."
                ),
                Say(
                    "When a type was wrong, it said why, and cited the manual. ty took the types from there into "
                    "every line of Python. Two published hooks, and one line of config, made sure no commit skips "
                    "any of it."
                ),
                Say("Captain Clear is invisible. His badge says so."),
                Hold(2),
            ],
        ),
    ]
)
