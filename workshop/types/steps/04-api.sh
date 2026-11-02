#!/usr/bin/env bash
# Step 4: the API speaks the query types; ty checks every parameter we bind.
source "$(dirname "$0")/lib.sh"

cat > main.py <<'PY'
import psycopg
from fastapi import FastAPI, HTTPException
from sqlty.drivers.psycopg import execute

import queries as q
from migrate import DATABASE_URL

app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")


def connect() -> psycopg.Connection:
    return psycopg.connect(DATABASE_URL, autocommit=True)


@app.get("/ducks")
def ducks() -> list[q.Duck]:
    with connect() as conn:
        return execute(conn, q.LIST_DUCKS).fetchall()


@app.get("/ducks/{duck_id}/badge")
def badge(duck_id: int) -> str:
    with connect() as conn:
        duck = execute(conn, q.DUCK.bind({"id": duck_id})).fetchone()
    if duck is None:
        raise HTTPException(404, "No such duck.")
    return f"{duck.name.upper()}: THE {duck.colour.upper()} DUCK"


@app.post("/consultations")
def book(booking: q.BookParams) -> q.Consultation | None:
    with connect() as conn:
        return execute(conn, q.BOOK.bind(booking)).fetchone()


@app.post("/consultations/{consultation_id}/solved")
def solve(consultation_id: int) -> q.Consultation:
    with connect() as conn:
        consultation = execute(conn, q.SOLVE.bind({"consultation_id": consultation_id})).fetchone()
    if consultation is None:
        raise HTTPException(404, "No such consultation. The duck is confused.")
    return consultation


@app.get("/leaderboard")
def leaderboard() -> list[q.Champion]:
    with connect() as conn:
        return execute(conn, q.LEADERBOARD).fetchall()
PY

say "the query names its parameter id, not consultation_id: ty knows, because sqlty wrote SolveParams"
expect_failure uv run ty check
sed -i 's/bind({"consultation_id": consultation_id})/bind({"id": consultation_id})/' main.py
uv run ty check
cat > conftest.py <<'PY'
import os

import psycopg

TEST_URL = "postgresql://duck:quack@localhost:5432/duckdesk_test"
os.environ["DATABASE_URL"] = TEST_URL

with psycopg.connect(TEST_URL, dbname="postgres", autocommit=True) as server:
    server.execute("DROP DATABASE IF EXISTS duckdesk_test WITH (FORCE)")

import migrate  # noqa: E402

migrate.migrate(TEST_URL)
PY

cat > test_main.py <<'PY'
from fastapi.testclient import TestClient

from main import app

client = TestClient(app)


def test_the_cheapest_duck_is_last() -> None:
    assert client.get("/ducks").json()[-1]["name"] == "Segfault Sam"


def test_badges_are_shouted() -> None:
    assert client.get("/ducks/2/badge").json() == "ADMIRAL BUBBLES: THE NAVY DUCK"


def test_the_body_is_the_parameters_of_the_query() -> None:
    assert client.post("/consultations", json={"duck_id": 2, "bug": "who am I?"}).status_code == 422


def test_the_hardest_bug_is_none_until_one_is_solved() -> None:
    assert client.get("/leaderboard").json()[0]["hardest_bug"] is None
    booking = client.post("/consultations", json={"duck_id": 2, "developer_id": 1, "bug": "works on my machine"})
    assert client.post(f"/consultations/{booking.json()['id']}/solved").json()["solved"] is True
    assert client.get("/leaderboard").json()[0] == {
        "name": "Admiral Bubbles",
        "bugs_solved": 1,
        "hardest_bug": "works on my machine",
    }
PY

uv run pytest -q
say "try it: uv run uvicorn main:app --reload, then open /docs"

commit "Step 4: the API"
