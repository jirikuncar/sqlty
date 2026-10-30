#!/usr/bin/env bash
# Step 7: two green branches, one red merge. Git merges text, not meaning; prek checks the meaning.
source "$(dirname "$0")/lib.sh"

say "you: ducks by colour"
git switch -q -c feature/ducks-by-colour main
cat >> queries.py <<'PY'

DUCKS_BY_COLOUR = SQL[Duck]("--sql\nSELECT id, name, colour, hourly_rate FROM ducks WHERE colour = %(colour)s")
PY
cat >> main.py <<'PY'


@app.get("/ducks/{colour}")
def ducks_by_colour(colour: str) -> list[q.Duck]:
    with connect() as conn:
        return execute(conn, q.DUCKS_BY_COLOUR, {"colour": colour}).fetchall()
PY
cat >> test_main.py <<'PY'


def test_the_navy_has_an_admiral() -> None:
    assert [duck["name"] for duck in client.get("/ducks/navy").json()] == ["Admiral Squeaks"]
PY
commit "Ducks by colour"

say "Bob: the style guide says American English"
git switch -q -c bob/american-spelling main
cat > migrations/003_american_spelling.sql <<'SQL'
ALTER TABLE ducks RENAME COLUMN colour TO color;
SQL
sed -i 's/colour/color/g' queries.py
as_bob commit "Use American spelling, per the style guide"

say "both branches are green; merge them"
git switch -q main
as_bob git merge -q --no-ff -m "Merge bob/american-spelling" bob/american-spelling
say "no conflicts, but prek runs on the merge commit too"
expect_failure git merge -q --no-ff -m "Merge feature/ducks-by-colour" feature/ducks-by-colour

say "the fix the editor offers: Quick Fix > Fix unknown-column (or sqlty check --fix)"
uv run sqlty check --fix .
git --no-pager diff --color=always | cat
git add -A
git commit -q --no-edit
say "merged, with the fix"
