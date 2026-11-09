"""Score the answers run.mjs wrote by running them on PostgreSQL against seeded data.

    DSN=postgresql://... uv run --with 'psycopg[binary]' python scripts/ask_eval/score.py ask-eval-*.json

A query is correct when its rows equal the gold query's (in order only when the question
asks for it), or, leniently, when it has every gold column among extra ones. `first` scores
the model's first query, `checked` the one after sqlty's problems were sent back.
"""

from __future__ import annotations

import datetime
import decimal
import json
import os
import random
import sys
from pathlib import Path

import psycopg

QUESTIONS = json.loads((Path(__file__).parent / "questions.json").read_text())


def database(schema: str) -> psycopg.Connection:
    rng = random.Random(7)
    db = psycopg.connect(os.environ["DSN"], autocommit=True)
    db.execute(f"DROP SCHEMA IF EXISTS {schema} CASCADE")
    db.execute(f"CREATE SCHEMA {schema}")
    db.execute(f"SET search_path TO {schema}")
    db.execute(QUESTIONS["schemas"][schema])
    day = datetime.datetime(2024, 1, 1)
    if schema == "shop":
        for id in range(1, 31):
            email = None if id % 7 == 0 else f"user{id}@{'example.com' if id % 3 else 'mail.org'}"
            db.execute("INSERT INTO users VALUES (%s, %s, %s)", [id, f"user {id}", email])
        for id in range(1, 201):
            placed = day + datetime.timedelta(minutes=rng.randrange(0, 2 * 365 * 24 * 60))
            db.execute(
                "INSERT INTO orders VALUES (%s, %s, %s, %s)",
                [
                    id,
                    rng.randrange(1, 26),
                    decimal.Decimal(rng.randrange(100, 100_000)) / 100,
                    placed,
                ],
            )
    else:
        for id in range(1, 21):
            db.execute(
                "INSERT INTO customers VALUES (%s, %s)",
                [id, rng.choice(["retail", "smb", "enterprise"])],
            )
        for id, name in enumerate(
            ["Widget", "Gadget", "Doohickey", "Gizmo", "Sprocket", "Unused"], 1
        ):
            db.execute("INSERT INTO products VALUES (%s, %s)", [id, name])
        item = payment = 0
        for id in range(1, 61):
            amounts = [
                decimal.Decimal(rng.randrange(100, 20_000)) / 100
                for _ in range(rng.randrange(0, 4))
            ]
            db.execute(
                "INSERT INTO orders VALUES (%s, %s, %s)",
                [id, rng.randrange(1, 21), sum(amounts, decimal.Decimal(0))],
            )
            for amount in amounts:
                item += 1
                db.execute(
                    "INSERT INTO order_items VALUES (%s, %s, %s, %s)",
                    [item, id, rng.randrange(1, 6), amount],
                )
            paid = sum(amounts, decimal.Decimal(0)) * rng.choice([0, decimal.Decimal("0.5"), 1])
            for share in (paid / 2, paid / 2) if paid else ():
                payment += 1
                db.execute(
                    "INSERT INTO payments VALUES (%s, %s, %s)", [payment, id, round(share, 2)]
                )
    return db


def normal(value):
    if isinstance(value, (decimal.Decimal, float)):
        return round(float(value), 2)
    return value if value is None or isinstance(value, (int, str)) else str(value)


def rows(db, sql):
    with db.cursor() as cursor:
        cursor.execute(sql.replace("%", "%%"))
        return [tuple(normal(value) for value in row) for row in cursor.fetchall()]


def key(row):
    return tuple((value is None, str(value)) for value in row)


def correct(db, item, sql) -> tuple[bool, bool]:
    try:
        found = rows(db, sql)
    except psycopg.Error:
        return False, False
    gold = rows(db, item["gold"])
    if not item.get("ordered"):
        found, gold = sorted(found, key=key), sorted(gold, key=key)
    if found == gold:
        return True, True
    if len(found) != len(gold) or not found:
        return False, False
    columns = [
        sorted(column, key=lambda value: key([value])) for column in zip(*found, strict=True)
    ]
    lenient = all(
        sorted(column, key=lambda value: key([value])) in columns
        for column in zip(*gold, strict=True)
    )
    return False, lenient


def main() -> None:
    dbs = {name: database(name) for name in QUESTIONS["schemas"]}
    for path in sys.argv[1:]:
        run = json.loads(Path(path).read_text())
        score = {"first": [0, 0], "checked": [0, 0]}
        clean = {"first": 0, "checked": 0}
        failures = []
        for answer in run["answers"]:
            db = dbs[answer["schema"]]
            for name, attempt in (
                ("first", answer["attempts"][0]),
                ("checked", answer["attempts"][-1]),
            ):
                strict, lenient = correct(db, answer, attempt["sql"])
                score[name][0] += strict
                score[name][1] += lenient
                clean[name] += not attempt["problems"]
            if not correct(db, answer, answer["sql"])[1]:
                failures.append(answer["question"])
        total = len(run["answers"])
        seconds = sorted(answer["seconds"] for answer in run["answers"])
        print(
            f"{run['model']} (isolated={run['isolated']}): load {run['loadSeconds']:.1f}s, "
            f"median {seconds[total // 2]:.1f}s/question; "
            f"first {score['first'][0]}/{total} exact, {score['first'][1]}/{total} lenient, {clean['first']} sqlty-clean; "
            f"checked {score['checked'][0]}/{total} exact, {score['checked'][1]}/{total} lenient, {clean['checked']} sqlty-clean"
        )
        for question in failures:
            print(f"  wrong: {question}")


if __name__ == "__main__":
    main()
