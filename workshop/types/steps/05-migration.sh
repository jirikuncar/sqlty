#!/usr/bin/env bash
# Step 5: a duck with no colour. The schema changes, and the types follow it into Python.
source "$(dirname "$0")/lib.sh"

cat > migrations/003_captain_clear.sql <<'SQL'
ALTER TABLE ducks ALTER COLUMN colour DROP NOT NULL;

INSERT INTO ducks (name, colour, hourly_rate) VALUES ('Captain Clear', NULL, 7.00);
SQL
uv run python migrate.py

say "every test passes: no test asks for the badge of Captain Clear"
uv run pytest -q

say "commit: it goes through. sqlty-check was skipped: the published hook runs on Python files, and a migration is SQL"
commit "Step 5: Captain Clear"

say "check the whole project on every commit, whatever changed"
sed -i 's/{ id = "sqlty-check" }/{ id = "sqlty-check", always_run = true, pass_filenames = false }/' prek.toml
cat prek.toml
git add -A
expect_failure git commit -q -m "Step 5: check every commit"

sed -i 's/    colour: str$/    colour: str | None/' queries.py
say "commit: sqlty is happy, ty is not; .upper() on None is a 500 in production"
git add -A
expect_failure git commit -q -m "Step 5: check every commit"

sed -i "s/{duck.colour.upper()}/{(duck.colour or 'invisible').upper()}/" main.py
cat >> test_main.py <<'PY'


def test_captain_clear_is_invisible() -> None:
    assert client.get("/ducks/4/badge").json() == "CAPTAIN CLEAR: THE INVISIBLE DUCK"
PY
uv run pytest -q

commit "Step 5: check every commit; Captain Clear is invisible"
