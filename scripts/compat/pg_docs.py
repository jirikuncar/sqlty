"""The examples of the PostgreSQL documentation, against a live server.

Reads every example of the function and operator tables of the "Functions and Operators"
chapter (`expression → result`) for the server's major version, prepares `SELECT expression`
on the server, and compares the type it describes with `core.analyze_select`. Writes
`reports/pg_docs.md`: the examples sqlty types differently (exit status 1), leaves untyped,
or can't parse.

    SQLTY_PG_DSN=postgresql://postgres:postgres@localhost:5432/postgres \\
        uv run python scripts/compat/pg_docs.py

The pages are fetched from postgresql.org, or read from a local copy of the HTML
documentation with `--docs DIR`.
"""

from __future__ import annotations

import argparse
import html
import os
import re
import sys
import urllib.request
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT))

import psycopg  # noqa: E402
from psycopg import pq  # noqa: E402

from sqlty import core  # noqa: E402
from tests.test_pg_describe import compare, load_pg_types  # noqa: E402

PAGE = re.compile(r'href="(functions-[a-z0-9-]+\.html)"')
EXAMPLE = re.compile(r'<p>\s*<code class="literal">((?:(?!</code>).)*)</code>\s*→', re.S)
TAG = re.compile(r"<[^>]+>")


@dataclass(frozen=True)
class Example:
    page: str
    query: str


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--docs", type=Path, help="a local copy of the HTML documentation")
    docs: Path | None = parser.parse_args().docs
    dsn = os.environ.get("SQLTY_PG_DSN")
    if not dsn:
        print("SQLTY_PG_DSN is not set", file=sys.stderr)
        return 2
    results: dict[str, list[str]] = {"mismatch": [], "untyped": [], "unparsed": [], "rejected": []}
    agree = 0
    with psycopg.connect(dsn, autocommit=True) as conn:
        conn.execute("SET statement_timeout = '5s'")
        major = conn.info.server_version // 10000
        types = load_pg_types(conn)
        examples = list(_examples(docs, major))
        for example in examples:
            server = _describe(conn, example.query)
            entry = f"| {example.page} | `{_cell(example.query)}`"
            if isinstance(server, str):
                results["rejected"].append(f"{entry} | {_cell(server)} |")
                continue
            ours = core.analyze_select(example.query, dialect="postgres")
            if not ours or len(ours) != len(server):
                results["unparsed"].append(f"{entry} |")
                continue
            for column, (name, oid) in zip(ours, server, strict=True):
                server_type, kind = types.get(oid, (str(oid), "b"))
                if column.python_type is None:
                    results["untyped"].append(f"{entry} | `{server_type}` |")
                elif why := compare(column, name, server_type, kind):
                    results["mismatch"].append(f"{entry} | {_cell(why)} |")
                else:
                    agree += 1
    report = [
        f"# PostgreSQL {major} documentation examples",
        "",
        f"{len(examples)} examples of the function and operator tables, "
        f"`scripts/compat/pg_docs.py`: {agree} columns agree with the server.",
        "",
        "## Typed differently",
        "",
        *_table(results["mismatch"], "Difference"),
        "## Untyped",
        "",
        *_table(results["untyped"], "Server type"),
        "## Not parsed",
        "",
        *_table(results["unparsed"]),
        "## Rejected by the server",
        "",
        "The example needs a table, type or role the documentation creates elsewhere.",
        "",
        *_table(results["rejected"], "Error"),
    ]
    (Path(__file__).parent / "reports" / "pg_docs.md").write_text("\n".join(report).rstrip() + "\n")
    print(
        ", ".join(f"{len(entries)} {kind}" for kind, entries in results.items())
        + f", {agree} agree"
    )
    return 1 if results["mismatch"] else 0


def _examples(docs: Path | None, major: int) -> Iterator[Example]:
    """Each example of the chapter, as a query, in page order."""
    if docs is None:
        base = f"https://www.postgresql.org/docs/{major}/"
        pages = sorted(set(PAGE.findall(_fetch(base + "functions.html"))))
        sources = [(page, _fetch(base + page)) for page in pages]
    else:
        sources = [(path.name, path.read_text()) for path in sorted(docs.glob("functions-*.html"))]
    for page, text in sources:
        for match in EXAMPLE.finditer(text):
            expression = " ".join(html.unescape(TAG.sub("", match.group(1))).split()).rstrip(";")
            if expression.lower().startswith("select "):
                yield Example(page, expression)
            else:
                yield Example(page, f"SELECT {expression}")


def _fetch(url: str) -> str:
    with urllib.request.urlopen(url, timeout=30) as response:  # noqa: S310
        text: str = response.read().decode()
    return text


def _describe(conn: Any, query: str) -> list[tuple[str, int]] | str:
    """The names and type OIDs of the query's columns, or the server's error."""
    pgconn = conn.pgconn
    result = pgconn.prepare(b"sqlty_docs", query.encode())
    if result.status != pq.ExecStatus.COMMAND_OK:
        return str(result.error_message.decode(errors="replace").strip().splitlines()[0])
    try:
        description = pgconn.describe_prepared(b"sqlty_docs")
        return [
            (description.fname(i).decode(), description.ftype(i))
            for i in range(description.nfields)
        ]
    finally:
        pgconn.exec_(b"DEALLOCATE sqlty_docs")


def _cell(text: str) -> str:
    return text.replace("|", "\\|").replace("`", "'")


def _table(entries: list[str], *columns: str) -> list[str]:
    if not entries:
        return ["None.", ""]
    header = ["Page", "Example", *columns]
    return [
        "| " + " | ".join(header) + " |",
        "|" + "|".join("---" for _ in header) + "|",
        *entries,
        "",
    ]


if __name__ == "__main__":
    raise SystemExit(main())
