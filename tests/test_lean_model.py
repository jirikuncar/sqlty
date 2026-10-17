"""The Lean model of the type mapping (``proofs/``) against sqlty, on random SQL types.

``proofs/`` proves that the model only gives well-formed Python types. This checks that
the model is the mapping: each case casts to a SQL type, reads back the type sqlty
resolved (``sql_type``) as the model's input, and compares the Python types. Needs
``lake build`` in ``proofs/``; the Lean model job in CI runs it.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from collections.abc import Iterator
from functools import cache
from pathlib import Path
from typing import Any

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from sqlty import core

PROOFS = Path(__file__).parent.parent / "proofs"
MODEL = PROOFS / ".lake" / "build" / "bin" / "sqlty-model"

if not MODEL.exists():  # pragma: no cover - the Lean model job builds it
    pytest.skip("run `lake build` in proofs/ to build the model", allow_module_level=True)

DIALECTS = ("postgres", "mysql", "sqlite", "duckdb")
EXAMPLES = int(os.environ.get("SQLTY_FUZZ_EXAMPLES", "300"))


@pytest.fixture(scope="module")
def model() -> Iterator[subprocess.Popen[str]]:
    with subprocess.Popen(
        [MODEL], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1
    ) as process:
        yield process
        assert process.stdin is not None
        process.stdin.close()


def test_scalar_types_match(model: subprocess.Popen[str]) -> None:
    mismatches = [
        (dialect, name, expected, actual)
        for dialect in DIALECTS
        for name in _scalars(dialect)
        if (expected := _sqlty(name, dialect)) is not None
        and (actual := _model(model, dialect, expected[0])) != expected[1]
    ]
    assert not mismatches


@settings(max_examples=EXAMPLES, deadline=None, suppress_health_check=[HealthCheck.too_slow])
@given(st.data())
def test_nested_types_match(model: subprocess.Popen[str], data: st.DataObject) -> None:
    dialect = data.draw(st.sampled_from(("postgres", "duckdb")))
    spelling = data.draw(_types(dialect))
    expected = _sqlty(spelling, dialect)
    if expected is None:
        return
    sql_type, python_type = expected
    assert _model(model, dialect, sql_type) == python_type, f"{dialect}: {spelling} is {sql_type}"


def _types(dialect: str) -> st.SearchStrategy[str]:
    scalar = st.sampled_from(_scalars(dialect))
    if dialect == "postgres":
        arrays = st.sampled_from(("[]", "[3]"))
        return st.recursive(
            scalar, lambda inner: st.tuples(inner, arrays).map("".join), max_leaves=3
        )

    def containers(inner: st.SearchStrategy[str]) -> st.SearchStrategy[str]:
        fields = st.lists(inner, min_size=1, max_size=3).map(
            lambda types: ", ".join(f"f{i} {t}" for i, t in enumerate(types))
        )
        return st.one_of(
            inner.map(lambda t: f"{t}[]"),
            inner.map(lambda t: f"{t}[3]"),
            fields.map(lambda f: f"STRUCT({f})"),
            fields.map(lambda f: f"UNION({f})"),
            st.tuples(scalar, inner).map(lambda kv: f"MAP({kv[0]}, {kv[1]})"),
        )

    return st.recursive(scalar, containers, max_leaves=6)


@cache
def _scalars(dialect: str) -> list[str]:
    names = re.findall(
        r'^  \(\.\w+, ("(?:[^"\\]|\\.)*")', (PROOFS / "SqlTy" / "Table.lean").read_text(), re.M
    )
    candidates = sorted({json.loads(name) for name in names}) + [
        "INTERVAL",
        "MYINTEGER",
        "MYTEXTISH",
        "WHATEVER",
    ]
    return [name for name in candidates if _sqlty(name, dialect) is not None]


@cache
def _schema(dialect: str) -> core.Schema:
    return core.load_schema("CREATE TABLE t (c TEXT NOT NULL)", dialect=dialect)


def _sqlty(spelling: str, dialect: str) -> tuple[str, str | None] | None:
    query = f"SELECT CAST(c AS {spelling}) AS c FROM t"
    columns = core.analyze_select(query, dialect=dialect, schema_sql=_schema(dialect))
    if not columns or columns[0].sql_type is None:
        return None
    return columns[0].sql_type, columns[0].python_type


def _model(model: subprocess.Popen[str], dialect: str, sql_type: str) -> str | None:
    assert model.stdin is not None and model.stdout is not None
    model.stdin.write(json.dumps({"dialect": dialect, "type": _structure(sql_type)}) + "\n")
    answer: str | None = json.loads(model.stdout.readline())
    return answer


def _structure(sql_type: str) -> Any:
    """The model's input for a type as sqlty spells it (``SqlType``'s ``Display``)."""
    sql_type = sql_type.strip()
    if sql_type == "INTERVAL":
        return "interval"
    if sql_type == "ARRAY":
        return {"array": None, "fixed": False}
    if match := re.fullmatch(r"ARRAY<(.*)>(\[\d+\])?", sql_type):
        return {"array": _structure(match[1]), "fixed": match[2] is not None}
    for kind in ("ROW", "STRUCT", "UNION", "MAP"):
        if sql_type.startswith(f"{kind}(") and sql_type.endswith(")"):
            parts = _split(sql_type[len(kind) + 1 : -1])
            if kind == "MAP":
                return {"map": [_structure(part) for part in parts]}
            return {kind.lower(): [_structure(_field_type(part, kind)) for part in parts]}
    return {"scalar": re.split(r"[(<]", sql_type, maxsplit=1)[0].strip()}


def _field_type(field: str, kind: str) -> str:
    name, _, sql_type = field.partition(" ")
    if kind == "ROW" and (not sql_type or not re.fullmatch(r"\w+", name)):
        return field
    return sql_type


def _split(text: str) -> list[str]:
    parts, depth, start = [], 0, 0
    for i, char in enumerate(text):
        if char in "(<":
            depth += 1
        elif char in ")>":
            depth -= 1
        elif char == "," and depth == 0:
            parts.append(text[start:i])
            start = i + 1
    parts.append(text[start:])
    return [part.strip() for part in parts]
