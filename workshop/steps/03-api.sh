#!/usr/bin/env bash
# Step 3: a FastAPI app over the typed queries, and tests on a fresh database.
source "$(dirname "$0")/lib.sh"

cat > main.py <<'PY'
import os

import psycopg
from fastapi import FastAPI
from pydantic import BaseModel
from sqlty.drivers.psycopg import execute

import queries as q
from migrate import DATABASE_URL

VERSION = os.environ.get("APP_VERSION", "dev")

app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")


def connect() -> psycopg.Connection:
    return psycopg.connect(DATABASE_URL, autocommit=True)


class Booking(BaseModel):
    duck_id: int
    developer_id: int
    bug: str


@app.get("/")
def motto() -> dict[str, str]:
    return {"motto": "Have you tried explaining it to the duck?", "version": VERSION}


@app.get("/ducks")
def ducks() -> list[q.Duck]:
    with connect() as conn:
        return execute(conn, q.LIST_DUCKS).fetchall()


@app.post("/consultations")
def book(booking: Booking) -> dict[str, int]:
    with connect() as conn:
        (consultation_id,) = execute(conn, q.BOOK, booking.model_dump()).fetchone()
    return {"id": consultation_id}


@app.post("/consultations/{consultation_id}/solved")
def solve(consultation_id: int) -> dict[str, str]:
    with connect() as conn:
        conn.execute(q.SOLVE, {"id": consultation_id})
    return {"duck": "quack"}


@app.get("/leaderboard")
def leaderboard() -> list[q.Champion]:
    with connect() as conn:
        return execute(conn, q.LEADERBOARD).fetchall()
PY

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


def test_a_solved_bug_crowns_the_duck() -> None:
    booking = client.post("/consultations", json={"duck_id": 2, "developer_id": 1, "bug": "works on my machine"})
    client.post(f"/consultations/{booking.json()['id']}/solved")
    assert client.get("/leaderboard").json()[0] == {"name": "Admiral Squeaks", "bugs_solved": 1}
PY

uv run pytest -q
say "try it: uv run uvicorn main:app --reload"

commit "Step 3: the API"
