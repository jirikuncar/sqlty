# /// script
# requires-python = ">=3.12"
# dependencies = ["pydantic-evals>=2.67", "psycopg[binary]", "playwright==1.64.1", "logfire"]
# ///
"""Evaluate the playground's ask on questions.json with pydantic-evals.

    DSN=postgresql://... uv run scripts/ask_eval/evals.py ask --model qwen2.5-coder-1.5b --variant rules --models DIR
    DSN=postgresql://... uv run scripts/ask_eval/evals.py score runs/*.json --markdown summary.md

`ask` asks a model in headless Chromium, as the playground does, and saves its answers;
`score` scores saved answers again. A query is `exact` when its rows equal the gold query's
(in order only when the question asks for it), `lenient` when it has every gold column among
extra ones; `first_exact` scores the model's first query, before sqlty's problems were sent back.
--models holds the GGUF files of ask.js MODELS and models.json, downloaded when missing; `make playground` builds the
engine and vendors wllama. .github/workflows/ask-eval.yml runs it on demand and monthly.
Spans go to Logfire when LOGFIRE_TOKEN is set.
"""

from __future__ import annotations

import argparse
import asyncio
import datetime
import decimal
import functools
import http.server
import json
import os
import random
import re
import statistics
import tempfile
import threading
import urllib.request
from contextlib import asynccontextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import logfire
import psycopg
from playwright.async_api import async_playwright
from pydantic_evals import Case, Dataset, increment_eval_metric
from pydantic_evals.evaluators import Evaluator, EvaluatorContext
from pydantic_evals.reporting import EvaluationReport
from rich.console import Console
from rich.table import Table

HERE = Path(__file__).parent
ROOT = HERE.parents[1]
DATA = json.loads((HERE / "questions.json").read_text())


@dataclass
class Question:
    schema: str
    question: str


@dataclass
class Answer:
    sql: str
    attempts: list[dict[str, Any]]
    seconds: float


@dataclass
class Rows(Evaluator[Question, Answer, dict[str, Any]]):
    def evaluate(self, ctx: EvaluatorContext[Question, Answer, dict[str, Any]]) -> dict[str, bool]:
        db = database(ctx.inputs.schema)
        ordered = ctx.metadata["ordered"]
        exact, lenient = match(db, ctx.output.sql, ctx.expected_output, ordered)
        first, _ = match(db, ctx.output.attempts[0]["sql"], ctx.expected_output, ordered)
        clean = not ctx.output.attempts[-1]["problems"]
        return {"exact": exact, "lenient": lenient, "first_exact": first, "sqlty_clean": clean}


DATASET = Dataset[Question, Answer, dict[str, Any]](
    name="ask",
    cases=[
        Case(
            name=item["question"],
            inputs=Question(item["schema"], item["question"]),
            expected_output=item["gold"],
            metadata={"ordered": item.get("ordered", False)},
        )
        for item in DATA["questions"]
    ],
    evaluators=[Rows()],
)


@functools.cache
def database(schema: str) -> psycopg.Connection:
    rng = random.Random(7)
    db = psycopg.connect(os.environ["DSN"], autocommit=True)
    db.execute(f"DROP SCHEMA IF EXISTS {schema} CASCADE")
    db.execute(f"CREATE SCHEMA {schema}")
    db.execute(f"SET search_path TO {schema}")
    db.execute(DATA["schemas"][schema])
    day = datetime.datetime(2024, 1, 1)
    if schema == "shop":
        for id in range(1, 31):
            email = None if id % 7 == 0 else f"user{id}@{'example.com' if id % 3 else 'mail.org'}"
            db.execute("INSERT INTO users VALUES (%s, %s, %s)", [id, f"user {id}", email])
        for id in range(1, 201):
            placed = day + datetime.timedelta(minutes=rng.randrange(0, 2 * 365 * 24 * 60))
            user = rng.randrange(1, 26)
            total = decimal.Decimal(rng.randrange(100, 100_000)) / 100
            db.execute("INSERT INTO orders VALUES (%s, %s, %s, %s)", [id, user, total, placed])
        return db
    for id in range(1, 21):
        segment = rng.choice(["retail", "smb", "enterprise"])
        db.execute("INSERT INTO customers VALUES (%s, %s)", [id, segment])
    for id, name in enumerate(["Widget", "Gadget", "Doohickey", "Gizmo", "Sprocket", "Unused"], 1):
        db.execute("INSERT INTO products VALUES (%s, %s)", [id, name])
    item = payment = 0
    for id in range(1, 61):
        amounts = [
            decimal.Decimal(rng.randrange(100, 20_000)) / 100 for _ in range(rng.randrange(0, 4))
        ]
        total = sum(amounts, decimal.Decimal(0))
        db.execute("INSERT INTO orders VALUES (%s, %s, %s)", [id, rng.randrange(1, 21), total])
        for amount in amounts:
            item += 1
            db.execute(
                "INSERT INTO order_items VALUES (%s, %s, %s, %s)",
                [item, id, rng.randrange(1, 6), amount],
            )
        paid = total * rng.choice([0, decimal.Decimal("0.5"), 1])
        for share in (paid / 2, paid / 2) if paid else ():
            payment += 1
            db.execute("INSERT INTO payments VALUES (%s, %s, %s)", [payment, id, round(share, 2)])
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


def match(db, sql, gold_sql, ordered) -> tuple[bool, bool]:
    try:
        found = rows(db, sql)
    except psycopg.Error:
        return False, False
    gold = rows(db, gold_sql)
    if not ordered:
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


def answered(found: dict[str, Any]) -> Answer:
    increment_eval_metric("seconds", found["seconds"])
    increment_eval_metric("attempts", len(found["attempts"]))
    return Answer(found["sql"], found["attempts"], found["seconds"])


def label(run: dict[str, Any]) -> str:
    flags = [flag for flag in ("thinking", "file") if run.get(flag)]
    return " ".join([run["model"], run.get("variant", "base"), *flags])


async def score(path: Path) -> EvaluationReport:
    run = json.loads(path.read_text())
    answers = {found["question"]: found for found in run["answers"]}
    dataset = DATASET.model_copy(
        update={"cases": [case for case in DATASET.cases if case.name in answers]}
    )
    return await dataset.evaluate(
        lambda question: answered(answers[question.question]),
        name=label(run),
        max_concurrency=1,
        progress=False,
    )


def catalog() -> dict[str, dict[str, Any]]:
    ask = (ROOT / "docs/assets/playground/ask.js").read_text()
    entries = re.findall(r'id: "([^"]+)",[^}]*?repo: "([^"]+)",[^}]*?file: "([^"]+)"', ask)
    offered = [{"id": id, "repo": repo, "file": file} for id, repo, file in entries]
    return {
        model["id"]: model for model in [*offered, *json.loads((HERE / "models.json").read_text())]
    }


def download(id: str, directory: Path) -> None:
    model = catalog()[id]
    path = directory / model["file"]
    if path.exists():
        return
    directory.mkdir(parents=True, exist_ok=True)
    partial = path.with_name(path.name + ".part")
    urllib.request.urlretrieve(
        f"https://huggingface.co/{model['repo']}/resolve/main/{model['file']}", partial
    )
    partial.rename(path)


class Handler(http.server.SimpleHTTPRequestHandler):
    mounts: dict[str, Path] = {}
    isolated = True

    def translate_path(self, path):
        path = super().translate_path(path)
        relative = "/" + os.path.relpath(path, os.getcwd())
        for prefix, directory in self.mounts.items():
            if relative.startswith(prefix):
                return str(directory / relative.removeprefix(prefix))
        return str(HERE / "missing")

    def end_headers(self):
        if self.isolated:
            self.send_header("Cross-Origin-Opener-Policy", "same-origin")
            self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()

    def log_message(self, *args):
        pass


Handler.extensions_map[".mjs"] = "text/javascript"


@asynccontextmanager
async def browser(args):
    # A persistent profile, as a person's browser has: a fresh context's storage quota is too
    # small to cache the 1 GB+ models. COOP/COEP lets wllama use threads.
    Handler.mounts = {
        "/playground/": ROOT / "docs/assets/playground",
        "/eval/": HERE,
        "/vendor/": ROOT / "docs/assets/vendor",
        "/models/": Path(args.models).resolve(),
    }
    Handler.isolated = not args.single_thread
    server = http.server.ThreadingHTTPServer(("localhost", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="ask-eval-") as profile:
        async with async_playwright() as playwright:
            context = await playwright.chromium.launch_persistent_context(
                profile, executable_path=args.executable
            )
            page = await context.new_page()
            page.on("pageerror", lambda error: print(error))
            await page.goto(f"http://localhost:{server.server_port}/eval/page.html")
            try:
                yield page
            finally:
                await context.close()
                server.shutdown()


async def ask(args) -> tuple[Path, EvaluationReport]:
    run: dict[str, Any] = {
        "model": args.model,
        "variant": args.variant,
        "thinking": args.thinking,
        "file": args.file,
        "isolated": not args.single_thread,
        "answers": [],
    }
    download(args.model, Path(args.models))
    async with browser(args) as page:
        run |= await page.evaluate(
            "options => window.load(options)",
            {"model": args.model, "threads": args.threads, "thinking": args.thinking},
        )

        async def task(question: Question) -> Answer:
            found = await page.evaluate(
                "options => window.ask(options)",
                {
                    "schema": question.schema,
                    "question": question.question,
                    "repairs": args.repairs,
                    "variant": args.variant,
                    "file": args.file,
                },
            )
            run["answers"].append({**question.__dict__, **found})
            return answered(found)

        dataset = DATASET.model_copy(update={"cases": DATASET.cases[: args.limit]})
        report = await dataset.evaluate(task, name=label(run), max_concurrency=1)
    out = Path(args.out or f"{label(run).replace(' ', '-')}.json")
    out.write_text(json.dumps(run, indent=2))
    return out, report


COLUMNS = ("run", "exact", "lenient", "first exact", "sqlty clean", "median s", "errors")


def summary(reports: list[EvaluationReport]) -> list[list[str]]:
    rows = []
    for report in reports:
        count = len(report.cases) + len(report.failures)
        passed = [
            f"{sum(case.assertions[name].value for case in report.cases)}/{count}"
            for name in ("exact", "lenient", "first_exact", "sqlty_clean")
        ]
        seconds = [case.metrics["seconds"] for case in report.cases]
        median = f"{statistics.median(seconds):.1f}" if seconds else "-"
        rows.append([report.name, *passed, median, str(len(report.failures))])
    return rows


def markdown(rows: list[list[str]]) -> str:
    lines = [COLUMNS, ["---"] * len(COLUMNS), *rows]
    return "".join(f"| {' | '.join(line)} |\n" for line in lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    scoring = commands.add_parser("score", help="score saved answers again")
    scoring.add_argument("runs", nargs="+", type=Path)
    scoring.add_argument("--cases", action="store_true", help="print each case")
    scoring.add_argument("--markdown", type=Path, help="also write the summary as a Markdown table")
    asking = commands.add_parser("ask", help="ask a model in headless Chromium")
    asking.add_argument("--model", required=True)
    asking.add_argument("--models", required=True, help="where the GGUF files are, or go")
    asking.add_argument(
        "--variant",
        default="model",
        help="an ask.js VARIANTS prompt; `model` is the model's own in ask.js MODELS, else rules",
    )
    asking.add_argument("--thinking", action="store_true", help="let Qwen3 reason first")
    asking.add_argument("--file", action="store_true", help="send the schema's source too")
    asking.add_argument("--repairs", type=int, default=2)
    asking.add_argument("--threads", type=int)
    asking.add_argument("--single-thread", action="store_true", help="omit COOP/COEP")
    asking.add_argument("--executable", help="a Chromium to use instead of Playwright's")
    asking.add_argument("--limit", type=int, help="ask only the first questions")
    asking.add_argument("--out")
    args = parser.parse_args()
    logfire.configure(send_to_logfire="if-token-present", console=False)
    if args.command == "ask":
        out, report = asyncio.run(ask(args))
        report.print(include_durations=False)
        print(f"answers saved to {out}")
        reports = [report]
    else:
        reports = [asyncio.run(score(path)) for path in args.runs]
        for report in reports if args.cases else ():
            report.print(include_durations=False)
    rows = summary(reports)
    table = Table(*COLUMNS)
    for row in rows:
        table.add_row(*row)
    Console().print(table)
    if args.command == "score" and args.markdown:
        args.markdown.write_text(markdown(rows))


if __name__ == "__main__":
    main()
