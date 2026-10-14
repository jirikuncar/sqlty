"""Deterministic generator of a large synthetic sqlty project.

The project is a Python package of many files, a few thousand queries and one
PostgreSQL schema, written into a directory at run time (nothing is committed).

Example:
    >>> import tempfile
    >>> with tempfile.TemporaryDirectory() as d:
    ...     stats = generate(d, files=20, queries=30, tables=10, seed=1)
    >>> stats["queries"]
    30
"""

from __future__ import annotations

import random
import sys
from pathlib import Path

PKG = "bigapp"
STATUSES = ("new", "active", "closed")

# Every kind exercises one analysis feature. ``{t}``/``{p}`` are the table and its
# parent table, ``{v}``/``{f}`` a view and a function over ``{t}``.
# Each entry: (name, sql, row, params) where ``row`` is a ``(style, fields)`` pair.
KINDS: list[tuple[str, str, list[tuple[str, str]] | None, str | None]] = [
    (
        "join",
        "SELECT a.id, a.name, b.name AS parent_name FROM {t} a JOIN {p} b ON b.id = a.parent_id WHERE a.score > 10 ORDER BY a.id",
        [("id", "int"), ("name", "str"), ("parent_name", "str")],
        None,
    ),
    (
        "left_join",
        "SELECT a.id, b.name AS parent_name, b.score FROM {t} a LEFT JOIN {p} b ON b.id = a.parent_id",
        [("id", "int"), ("parent_name", "str | None"), ("score", "int | None")],
        None,
    ),
    (
        "right_join",
        "SELECT a.name AS name, b.id AS parent_id FROM {t} a RIGHT JOIN {p} b ON b.id = a.parent_id",
        [("name", "str | None"), ("parent_id", "int")],
        None,
    ),
    (
        "full_join",
        "SELECT a.id AS a_id, b.id AS b_id FROM {t} a FULL OUTER JOIN {p} b ON b.id = a.parent_id",
        [("a_id", "int | None"), ("b_id", "int | None")],
        None,
    ),
    (
        "cte",
        "WITH recent AS (SELECT id, name, status FROM {t} WHERE created_at > now() - interval '7 days') SELECT r.id, r.name, r.status FROM recent r",
        [("id", "int"), ("name", "str"), ("status", "Literal['new', 'active', 'closed']")],
        None,
    ),
    (
        "recursive_cte",
        "WITH RECURSIVE chain AS (SELECT id, parent_id FROM {t} WHERE parent_id IS NULL UNION ALL SELECT c.id, c.parent_id FROM {t} c JOIN chain p ON c.parent_id = p.id) SELECT id FROM chain",
        [("id", "int")],
        None,
    ),
    (
        "lateral",
        "SELECT p.id, l.name FROM {p} p LEFT JOIN LATERAL (SELECT c.name FROM {t} c WHERE c.parent_id = p.id ORDER BY c.id LIMIT 1) l ON true",
        [("id", "int"), ("name", "str | None")],
        None,
    ),
    (
        "json",
        "SELECT p.id, jsonb_agg(jsonb_build_object('id', c.id, 'name', c.name)) AS children FROM {p} p JOIN {t} c ON c.parent_id = p.id GROUP BY p.id",
        [("id", "int"), ("children", "Any")],
        None,
    ),
    (
        "aggregate",
        "SELECT status, count(*) AS n, max(score) AS best FROM {t} GROUP BY status HAVING count(*) > 1",
        [("status", "Literal['new', 'active', 'closed']"), ("n", "int"), ("best", "int")],
        None,
    ),
    (
        "literal",
        "SELECT id, status FROM {t} WHERE status = 'active'",
        [("id", "int"), ("status", "Literal['new', 'active', 'closed']")],
        None,
    ),
    (
        "view",
        "SELECT id, name FROM {v} WHERE id > 100",
        [("id", "int"), ("name", "str")],
        None,
    ),
    (
        "function",
        "SELECT {f}(1) AS n, count(*) AS k FROM {v}",
        [("n", "int"), ("k", "int")],
        None,
    ),
    (
        "subquery",
        "SELECT id, name FROM {t} WHERE id IN (SELECT parent_id FROM {t} WHERE parent_id IS NOT NULL)",
        [("id", "int"), ("name", "str")],
        None,
    ),
    (
        "params",
        "SELECT id, name FROM {t} WHERE id = %s AND score > %s",
        [("id", "int"), ("name", "str")],
        "(1, 2)",
    ),
    (
        "named_params",
        "SELECT id FROM {t} WHERE name = %(name)s AND status = %(status)s",
        [("id", "int")],
        "{'name': 'x', 'status': 'new'}",
    ),
    (
        "array_param",
        "SELECT id FROM {t} WHERE id = ANY(%s)",
        [("id", "int")],
        "([1, 2],)",
    ),
]

# Row-type styles: how the fields become a Python type.
STYLES = ("typeddict", "dataclass", "tuple", "tuple", "helper", "execute", "execute", "bare")


def schema_sql(tables: int, seed: int = 0) -> str:
    """Schema of ``tables`` tables with ALTERs, views, enums, CHECKs and functions."""
    rng = random.Random(seed)
    out = [
        "CREATE SCHEMA app;",
        "CREATE TYPE app.status AS ENUM ('new', 'active');",
        "ALTER TYPE app.status ADD VALUE 'closed';",
        "CREATE DOMAIN app.percent AS integer CHECK (VALUE BETWEEN 0 AND 100);",
    ]
    for i in range(tables):
        parent = f"app.t{i - 1:04d}" if i else f"app.t{i:04d}"
        out.append(
            f"CREATE TABLE app.t{i:04d} ("
            "id integer PRIMARY KEY, "
            f"parent_id integer REFERENCES {parent}(id), "
            "name text NOT NULL, "
            "status app.status NOT NULL DEFAULT 'new', "
            "score integer CHECK (score >= 0), "
            "created_at timestamptz NOT NULL DEFAULT now(), "
            "tags text[], payload jsonb, "
            "CONSTRAINT t%04d_name_len CHECK (length(name) < 200));" % i
        )
        out.append(f"ALTER TABLE app.t{i:04d} ALTER COLUMN score SET NOT NULL;")
        if rng.random() < 0.4:
            out.append(f"ALTER TABLE app.t{i:04d} ADD COLUMN note text;")
        if rng.random() < 0.2:
            out.append(f"ALTER TABLE app.t{i:04d} ADD COLUMN pct app.percent;")
        if rng.random() < 0.1:
            out.append(f"ALTER TABLE app.t{i:04d} RENAME COLUMN tags TO labels;")
            out.append(f"ALTER TABLE app.t{i:04d} RENAME COLUMN labels TO tags;")
        out.append(
            f"CREATE VIEW app.v{i:04d} AS SELECT id, name FROM app.t{i:04d} WHERE score >= 0;"
        )
        out.append(
            f"CREATE FUNCTION app.f{i:04d}(integer) RETURNS bigint LANGUAGE sql "
            f"AS 'SELECT count(*) FROM app.t{i:04d}';"
        )
    out.append("SET search_path = app, public;")
    return "\n".join(out) + "\n"


def unresolved_alter_schema(tables: int = 2000) -> str:
    """The known schema-load case: tables, then ALTERs on tables that do not exist."""
    out = [
        f"CREATE TABLE t{i} (id integer PRIMARY KEY, name text NOT NULL);" for i in range(tables)
    ]
    out += [f"ALTER TABLE missing{i} ADD COLUMN extra text;" for i in range(tables)]
    return "\n".join(out) + "\n"


def _row_type(style: str, name: str, fields: list[tuple[str, str]]) -> tuple[str, str]:
    """Class definition (may be empty) and the type expression for a row."""
    if style == "typeddict":
        body = "\n".join(f"    {f}: {t}" for f, t in fields)
        return f"class {name}(TypedDict):\n{body}\n\n", name
    if style == "dataclass":
        body = "\n".join(f"    {f}: {t}" for f, t in fields)
        return f"@dataclass\nclass {name}:\n{body}\n\n", name
    return "", "tuple[" + ", ".join(t for _, t in fields) + "]"


def _query(rng: random.Random, tables: int, n: int, idx: int) -> tuple[str, str]:
    """One query as ``(class_defs, statement)``."""
    kind, sql, fields, params = KINDS[rng.randrange(len(KINDS))]
    i = rng.randrange(1, tables)
    sql = sql.format(t=f"t{i:04d}", p=f"t{i - 1:04d}", v=f"v{i:04d}", f=f"f{i:04d}")
    assert fields is not None
    style = STYLES[rng.randrange(len(STYLES))]
    if params and style in {"helper", "typeddict", "dataclass", "tuple"}:
        style = "execute"
    var = f"q{idx}"
    literal = f'"--sql\\n{sql}"'
    plain = f'"{sql}"'
    if style == "bare":
        return "", f"{var} = {literal}\n"
    if style == "execute":
        arg = f", {params}" if params else ""
        return "", f"execute({plain}{arg})\n"
    if style == "helper":
        cls, ty = _row_type("typeddict", f"Row{n}_{idx}", fields)
        return cls, f"{var} = fetch_all({plain}, {ty})\n"
    cls, ty = _row_type(style, f"Row{n}_{idx}", fields)
    return cls, f"{var} = SQL[{ty}]({literal})\n"


def generate(
    root: str | Path,
    *,
    files: int = 10_000,
    queries: int = 3_000,
    tables: int = 500,
    seed: int = 42,
) -> dict[str, int]:
    """Write the project under ``root`` and return its size.

    Example:
        >>> import tempfile, pathlib
        >>> with tempfile.TemporaryDirectory() as d:
        ...     s = generate(d, files=5, queries=8, tables=4)
        ...     (pathlib.Path(d) / "schema.sql").exists()
        True
    """
    rng = random.Random(seed)
    root = Path(root)
    pkg = root / PKG
    pkg.mkdir(parents=True, exist_ok=True)
    (root / "pyproject.toml").write_text(
        '[tool.sqlty]\ndialect = "postgres"\nschema = "schema.sql"\n'
    )
    (root / "schema.sql").write_text(schema_sql(tables, seed))
    (pkg / "__init__.py").write_text("")
    (pkg / "db.py").write_text(
        "from typing import Annotated, LiteralString\n\n"
        "from sqlty import SQL\n\n\n"
        "def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T], /, **params: object) -> list[T]: ...\n\n\n"
        "def execute(query: Annotated[LiteralString, SQL], params: tuple[object, ...] | dict[str, object] = (), /) -> None: ...\n"
    )
    # Spread the queries over a random subset of the files; the rest hold plain code.
    per_file = [0] * files
    for _ in range(queries):
        per_file[rng.randrange(files)] += 1
    total_q = 0
    for n in range(files):
        header = [
            "from __future__ import annotations\n",
            "from dataclasses import dataclass\n",
            "from typing import Any, Literal, TypedDict\n",
            "from sqlty import SQL\n",
            f"from {PKG}.db import execute, fetch_all\n\n\n",
        ]
        classes: list[str] = []
        stmts: list[str] = []
        for j in range(per_file[n]):
            cls, stmt = _query(rng, tables, n, j)
            classes.append(cls)
            stmts.append(stmt)
            total_q += 1
        plain = (
            f"def handler_{n}(x: int) -> int:\n"
            f"    values = [i * {n % 7 + 1} for i in range(x)]\n"
            f"    return sum(values) + {n}\n"
        )
        body = "".join(classes) + "".join(stmts) + ("\n\n" if stmts else "") + plain
        (pkg / f"mod{n:05d}.py").write_text("".join(header) + body)
    return {"files": files, "queries": total_q, "tables": tables}


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "synth-project"
    print(generate(out))
