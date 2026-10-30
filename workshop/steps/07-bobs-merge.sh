#!/usr/bin/env bash
# Step 7: two green branches, one red main. Git merges text, not meaning.
source "$(dirname "$0")/lib.sh"

say "you: ducks by colour"
git switch -q -c feature/ducks-by-colour main
cat >> queries.py <<'PY'

DUCKS_BY_COLOUR = SQL[Duck]("--sql\nSELECT id, name, colour, hourly_rate FROM ducks WHERE colour = :colour")
PY
cat >> main.py <<'PY'


@app.get("/ducks/{colour}")
def ducks_by_colour(colour: str) -> list[q.Duck]:
    return fetch_all(q.DUCKS_BY_COLOUR, colour=colour)
PY
cat >> test_main.py <<'PY'


def test_the_navy_has_an_admiral() -> None:
    assert [duck["name"] for duck in client.get("/ducks/navy").json()] == ["Admiral Squeaks"]
PY
commit "Ducks by colour"

say "Bob: the style guide says American English"
git switch -q -c bob/american-spelling main
sed -i 's/colour/color/g' schema.sql seed.sql queries.py
as_bob commit "Use American spelling, per the style guide"

say "both pull requests are green; merge them"
git switch -q main
as_bob git merge -q --no-ff -m "Merge bob/american-spelling" bob/american-spelling
git merge -q --no-ff -m "Merge feature/ducks-by-colour" feature/ducks-by-colour
say "no conflicts. Is main green?"
if uv run sqlty check .; then
  echo "expected main to be red" >&2
  exit 1
fi

say "the fix the editor offers: Quick Fix > Fix unknown-column (or sqlty check --fix)"
uv run sqlty check --fix .
git --no-pager diff --color=always | cat
commit "Ducks by color"
