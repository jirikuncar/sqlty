#!/usr/bin/env bash
# Step 3: a FastAPI app over the typed queries, and a test.
source "$(dirname "$0")/lib.sh"

cat > main.py <<'PY'
import os
import sqlite3
from pathlib import Path

from fastapi import FastAPI
from pydantic import BaseModel
from sqlty import SQL, row_form

import queries as q

HERE = Path(__file__).parent

db = sqlite3.connect(os.environ.get("DUCKDESK_DB", ":memory:"), check_same_thread=False, autocommit=True)
db.executescript((HERE / "schema.sql").read_text() + (HERE / "seed.sql").read_text())

app = FastAPI(title="DuckDesk", description="Rubber ducks, rented by the hour.")


def fetch_all[T](query: SQL[T], **params: object) -> list[T]:
    row = row_form(query)
    return [row(*values) for values in db.execute(query, params)]


class Booking(BaseModel):
    duck_id: int
    developer_id: int
    bug: str


@app.get("/")
def motto() -> dict[str, str]:
    return {"motto": "Have you tried explaining it to the duck?"}


@app.get("/ducks")
def ducks() -> list[q.Duck]:
    return fetch_all(q.LIST_DUCKS)


@app.post("/consultations")
def book(booking: Booking) -> dict[str, int]:
    (consultation_id,) = db.execute(q.BOOK, booking.model_dump()).fetchone()
    return {"id": consultation_id}


@app.post("/consultations/{consultation_id}/solved")
def solve(consultation_id: int) -> dict[str, str]:
    db.execute(q.SOLVE, {"id": consultation_id})
    return {"duck": "quack"}


@app.get("/leaderboard")
def leaderboard() -> list[q.Champion]:
    return fetch_all(q.LEADERBOARD)
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
