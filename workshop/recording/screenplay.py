"""The recorded talk: narration, typing and commands, scene by scene."""

from kit import Fix, Hold, Keys, Open, Run, Say, Scene, Write, code


def commit(message: str, ok: bool | None = True) -> Run:
    return Run(f"git add -A && git commit -qm '{message}'", ok=ok)


def prek_with_deploy() -> str:
    hooks = code("05", "prek.toml").replace(
        'default_install_hook_types = ["pre-commit", "pre-merge-commit"]',
        'default_install_hook_types = ["pre-commit", "pre-merge-commit", "pre-push"]',
    )
    return hooks + (
        '\n[[repos.hooks]]\nid = "deploy"\nname = "deploy"\nentry = "./deploy.sh"\nlanguage = "system"\n'
        'pass_filenames = false\nalways_run = true\nstages = ["pre-push"]\n'
    )


README = """# DuckDesk

Rubber ducks, rented by the hour.

- Talks to every developer
- Never interrupts
- Deploys on Friday
"""

SCENES = [
    Scene(
        "open",
        [
            Say(
                "Every developer has a rubber duck. You explain your bug to the duck. The duck says nothing. "
                "And halfway through your sentence, you find the bug."
            ),
            Write("README.md", README),
            Say(
                "Ducks are the best debugging tool ever made. But nobody sells them as a service. Until today. "
                "This is DuckDesk: rubber ducks, rented by the hour."
            ),
            Say(
                "We meet investors in 20 minutes. They want a product that works, runs in production, and ships on "
                "every push. We have no money for a CI server, so everything runs on this laptop. And it's Friday. "
                "Developers say: never deploy on a Friday. Let's go."
            ),
        ],
    ),
    Scene(
        "setup",
        [
            Run("git init -q -b main"),
            Run("uv init -q --bare --app --name duckdesk --python 3.14"),
            Say(
                "uv gives us Python and a lockfile. The app is FastAPI and Postgres. sqlty checks our SQL, "
                "ty checks our Python, and prek runs git hooks. By the end, prek is our whole pipeline."
            ),
            Run(
                'uv add -q fastapi uvicorn "psycopg[binary]" "logfire[fastapi,psycopg]" "sqlty>=0.3.0"'
            ),
            Run("uv add -q --dev pytest httpx2 prek ruff ty"),
            Write(".gitignore", ".venv/\n__pycache__/\n", typed=False),
            Write("pyproject.toml", code("00", "pyproject.toml"), typed=False, where="append"),
            commit("An empty DuckDesk"),
        ],
    ),
    Scene(
        "migrations",
        [
            Write("compose.yaml", code("01", "compose.yaml"), typed=False),
            Run("docker compose up -d --quiet-pull --wait db 2>/dev/null"),
            Say(
                "Three tables. Ducks: a name, a colour, and a price per hour. Colour, with a U. That is British "
                "spelling, because our best duck is British: Sir Quack."
            ),
            Write("migrations/001_tables.sql", code("01", "migrations/001_tables.sql")),
            Say(
                "Developers are our customers. Consultations say which duck heard which bug, and if the bug is solved."
            ),
            Write("migrations/002_ducks.sql", code("01", "migrations/002_ducks.sql"), typed=False),
            Say(
                "Our ducks: Sir Quack costs 99 an hour. Admiral Bubbles costs 42. Segfault Sam is free. "
                "He crashes a lot."
            ),
            Write("migrate.py", code("01", "migrate.py"), typed=False),
            Say(
                "A small migrator: each file runs once, in order, in a transaction. One rule: when a migration has "
                "run in production, we never change it. Remember this rule. We need it later."
            ),
            Write("pyproject.toml", code("01", "pyproject.toml"), where="append"),
            Say(
                "sqlty reads the migrations folder as the schema. It never connects to the database. It reads the "
                "SQL files, ALTER TABLE too, and knows what the tables look like now."
            ),
            Run("uv run python migrate.py"),
            commit("Ducks, developers, consultations"),
        ],
    ),
    Scene(
        "queries",
        [
            Write(
                "pyproject.toml",
                "param-types = true\n",
                where="after",
                anchor='schema = "migrations/"',
            ),
            Say(
                "A query is a normal string. The first line is dash dash SQL. For the database, that line is a "
                "comment. For sqlty, it marks a query."
            ),
            Write("queries.py", code("02", "queries.py", 0)),
            Say(
                "Booking a duck is an INSERT. Solving a bug is an UPDATE. Both use RETURNING, so they give back the row."
            ),
            Run("uv run sqlty rewrite ."),
            Hold(),
            Say(
                "I did not write these types. sqlty read the migrations. hourly_rate is NUMERIC NOT NULL, so it is "
                "a Decimal, never None. Money in a float is how startups lose money."
            ),
            Say(
                "The second type is the parameters. BookParams was generated: duck_id is an int, not None, because "
                "the column is NOT NULL. And the query is still a normal string, so psycopg takes it."
            ),
            Write("queries.py", code("02", "queries.py", 1), typed=False),
            Run("uv run sqlty rewrite . && uv run ruff format -q queries.py"),
            Say(
                "Names are nicer than tuples. sqlty keeps my dataclasses, and checks every field against its column: "
                "name, type, and None or not."
            ),
            commit("Typed queries"),
        ],
    ),
    Scene(
        "api",
        [
            Write("main.py", code("03", "main.py"), typed=False),
            Say(
                "The request body is BookParams: the parameters of the query. FastAPI checks it and puts it in the "
                "API docs. So the API contract comes from the SQL. bind checks the parameters, and ty checks bind."
            ),
            Write("conftest.py", code("03", "conftest.py"), typed=False),
            Write("test_main.py", code("03", "test_main.py"), typed=False),
            Say(
                "Each test run starts with a new, empty database. The test books Admiral Bubbles for: works on my "
                "machine. In real life, works on my machine is never solved. In tests, everything is."
            ),
            Run("uv run pytest -q"),
            Run("uv run ty check"),
            commit("The API"),
        ],
    ),
    Scene(
        "logfire",
        [
            Write("main.py", "import logfire\n", where="before", anchor="import psycopg"),
            Write(
                "main.py",
                '\nlogfire.configure(service_name="duckdesk", service_version=VERSION, send_to_logfire="if-token-present")\n'
                "logfire.instrument_psycopg()\n",
                where="after",
                anchor='VERSION = os.environ.get("APP_VERSION", "dev")',
            ),
            Say(
                "Three lines, and we have OpenTelemetry traces. Without a token, nothing leaves this laptop. "
                "Remember service_version: we need it later."
            ),
            Write(
                "main.py",
                "logfire.instrument_fastapi(app)\n",
                where="after",
                anchor="app = FastAPI(",
            ),
            Write(
                "main.py",
                '    logfire.info("duck {duck_id} hears about {bug!r}", **booking)\n',
                where="after",
                anchor="def book(",
            ),
            Run("uv run uvicorn main:app", pane="server", wait=False),
            Hold(3),
            Run(
                "curl -s localhost:8000/consultations -H 'content-type: application/json' "
                """-d '{"duck_id": 1, "developer_id": 2, "bug": "it is always DNS"}'"""
            ),
            Say(
                "Here is the request, the SQL inside it, and our log. Sir Quack replies very fast. He never says anything."
            ),
            Run("^C", pane="server", wait=False),
            commit("Logfire"),
        ],
    ),
    Scene(
        "prek",
        [
            Write("prek.toml", code("05", "prek.toml"), typed=False),
            Run("uv run prek install"),
            Say(
                "prek runs ruff, sqlty, ty and the tests. It runs before every commit, and before every merge commit. "
                "Remember the merge."
            ),
            commit("prek"),
            Say(
                "Investors love a backlog. I'm in a hurry, so the type says str. The editor shows a red line. "
                "I'm in a hurry. I ignore it."
            ),
            Write("queries.py", code("05", "queries.py"), where="append"),
            Write("main.py", code("05", "main.py"), typed=False, where="append"),
            commit("The backlog", ok=False),
            Hold(),
            Say(
                "The tests pass. No test calls the backlog, so the first real user finds out, in production. "
                "ty can't see it, because the SQL is a string. sqlty reads the string."
            ),
            Open("queries.py"),
            Keys(":%s/open_bugs: str/open_bugs: int/<CR>:w<CR>"),
            commit("The backlog, typed"),
        ],
    ),
    Scene(
        "deploy",
        [
            Write("Dockerfile", code("06", "Dockerfile"), typed=False),
            Write("deploy.sh", code("06", "deploy.sh"), typed=False),
            Say(
                "This is our deploy server. It builds the image from the commit. It runs the migrations. It starts "
                "the new version next to the old one. Both versions run for a few seconds. Then we stop the old one, "
                "and tag the commit prod."
            ),
            Write("traffic.sh", code("06", "traffic.sh"), typed=False),
            Write("prek.toml", prek_with_deploy(), typed=False),
            Run("chmod +x deploy.sh traffic.sh && uv run prek install"),
            Say(
                "prek runs it before git push. So git push is our deploy button. When the deploy fails, the push fails."
            ),
            commit("Push to deploy"),
            Run(
                "git init -q --bare ../duckdesk-origin.git && git remote add origin ../duckdesk-origin.git"
            ),
            Run("git push -q -u origin main"),
            Run("./traffic.sh", pane="server", wait=False),
            Say("Version one is live. Now, let me introduce Bob."),
        ],
    ),
    Scene(
        "merge",
        [
            Say(
                "Bob is our second developer. Bob reads the style guide. The style guide says: American English. "
                "Bob follows every rule."
            ),
            Run("git switch -q -c feature/ducks-by-colour"),
            Write("queries.py", code("07", "queries.py"), where="append"),
            Write("main.py", code("07", "main.py"), typed=False, where="append"),
            Write("test_main.py", code("07", "test_main.py"), typed=False, where="append"),
            Run("uv run sqlty rewrite -q . && uv run ruff format -q queries.py"),
            commit("Ducks by colour"),
            Say(
                "On his branch, Bob writes migration 003. He drops the U: colour becomes color. "
                "He updates his queries too."
            ),
            Run("git switch -q -c bob/american-spelling main"),
            Write(
                "migrations/003_american_spelling.sql",
                code("07", "migrations/003_american_spelling.sql"),
            ),
            Run("sed -i 's/colour/color/g' queries.py"),
            commit("American spelling"),
            Say("His branch is green. Mine is green. Merge Bob's, then mine."),
            Run("git switch -q main && git merge -q --no-ff -m 'Merge Bob' bob/american-spelling"),
            Run(
                "git merge -q --no-ff -m 'Merge ducks by colour' feature/ducks-by-colour", ok=False
            ),
            Hold(),
            Say(
                "Git merges text, not meaning. My new query reads colour, with a U. That column is gone on Bob's "
                "branch. Neither branch was wrong. Only the two together. The merge hook catches it, "
                "and the merge stays open."
            ),
            Open("queries.py"),
            Say(
                "The editor runs the same sqlty as the hook. The red line is under colour: did you mean color, "
                "without the U? One quick fix."
            ),
            Fix("queries.py"),
            Run("git add -A && git commit -q --no-edit"),
        ],
    ),
    Scene(
        "rolling",
        [
            Say(
                "Everything is green. Ship it? Look at the traffic. Version one still runs, and it reads colour, "
                "with a U. The migration runs first. It renames the column. And version one fails on every request, "
                "until the new version is up."
            ),
            Say(
                "Every check looked at one version of the code. A deploy runs two versions at once. sqlty breaking "
                "checks the code in production against the new schema."
            ),
            Write(
                "deploy.sh",
                "if git rev-parse -q --verify prod >/dev/null; then\n"
                '  uv run sqlty breaking --against prod --head "$ref" --direction backward\n'
                "fi\n",
                where="before",
                anchor='git archive "$ref" | docker build',
            ),
            commit("Check prod before deploying"),
            Run("git push -q", ok=False),
            Hold(),
            Say(
                "Refused: production reads colour. Migration 003 never ran in production. Only in test databases "
                "that we throw away. So we can still change it. We add the new column next to the old one, "
                "and copy the data."
            ),
            Write(
                "migrations/003_american_spelling.sql",
                code("08", "migrations/003_american_spelling.sql"),
            ),
            commit("Expand: color next to colour"),
            Run("git push -q"),
            Say(
                "Two versions, one database, both fine. The old one says colour, the new one says color. Logfire "
                "tells them apart by service_version. Nobody adds ducks during the deploy, so one copy of the data "
                "is enough. Next week, migration 004 drops colour."
            ),
        ],
    ),
    Scene(
        "wrap",
        [
            Say(
                "In 20 minutes: types from migrations, traces in three lines, a hook that caught what the tests "
                "missed, a merge that stopped, an editor that fixed it, and a deploy that kept production alive."
            ),
            Say(
                "We used no CI server. Sir Quack lost his U. He is still not talking to Bob. "
                "DuckDesk is now open for investment. We accept ducks."
            ),
            Hold(2),
        ],
    ),
]

for scene in SCENES:
    lines = [action for action in scene.actions if isinstance(action, Say)]
    for number, line in enumerate(lines, 1):
        line.id = f"{scene.name}-{number:02}"
