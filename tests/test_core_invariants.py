"""Properties of ``sqlty.core`` that hold for every input.

Values are specified in Markdown (``tests/mdtest``, ``docs``); this module
checks invariants over all of that code, and what Markdown can't express
(exceptions, CRLF, timing).
"""

from __future__ import annotations

import re
import time
from pathlib import Path
from typing import Any

import pytest

import sqlty.core as core
from tests._mdtest.parser import parse

ROOT = Path(__file__).resolve().parent.parent
MARKDOWN = [
    *sorted((ROOT / "tests" / "mdtest").rglob("*.md")),
    *sorted((ROOT / "docs").rglob("*.md")),
]


def _corpus(kind: str) -> list[Any]:
    """Every mdtest's SQL query or Python code, with its options."""
    params = []
    for md in [*MARKDOWN, ROOT / "README.md"]:
        for section in parse(md):
            settings = section.settings
            options = {
                "dialect": settings.get("dialect", "postgres"),
                "schema_sql": section.files.get(settings.get("schema", "")),
                "import_style": settings.get("import-style"),
                "untyped": settings.get("untyped") == "sentinel",
            }
            units = [u for u in section.units if u.lang == kind]
            for i, unit in enumerate(units):
                name = f"{md.relative_to(ROOT)}::{section.name}" + (f"::{i}" if i else "")
                params.append(pytest.param(unit.code.text, options, id=name))
    return params


PYTHON = _corpus("python")
SQL = _corpus("sql")


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_rewrite_is_idempotent(source: str, options: dict) -> None:
    once = core.rewrite_source(source, **options)
    assert core.rewrite_source(once, **options) == once


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_strip_is_idempotent(source: str, options: dict) -> None:
    once = core.strip_source(source, import_style=options["import_style"])
    assert core.strip_source(once, import_style=options["import_style"]) == once


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_strip_undoes_rewrite(source: str, options: dict) -> None:
    """``strip(rewrite(x)) == x`` for sources without sqlty wrappers or imports."""
    style = options["import_style"]
    if (
        re.search(r"^(from|import) sqlty", source, re.M)
        or core.strip_source(source, import_style=style) != source
    ):
        pytest.skip("source already uses sqlty")
    rewritten = core.rewrite_source(source, **options)
    assert core.strip_source(rewritten, import_style=style) == source


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_fix_is_idempotent(source: str, options: dict) -> None:
    once = core.fix_source(source, **options)
    assert core.fix_source(once, **options) == once


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_fix_leaves_nothing_stale(source: str, options: dict) -> None:
    """After ``--fix`` only unfixable diagnostics remain: unwrapped or stale queries can't."""
    fixed = core.fix_source(source, **options)
    stale = [
        d
        for d in core.check_source(
            fixed,
            dialect=options["dialect"],
            schema_sql=options["schema_sql"],
            import_style=options["import_style"],
            untyped=options["untyped"],
        )
        if d["rule"] in ("stale-rewrite", "stale-type")
    ]
    assert not stale


def test_unknown_schema_column_raises() -> None:
    with pytest.raises(RuntimeError, match="Column 'nope' not found in table 'users'"):
        core.infer_type("SELECT SUM(nope) FROM users", schema_sql="CREATE TABLE users (id INT);")


def test_crlf_is_kept() -> None:
    source = 'q = "--sql\\nSELECT 1::INTEGER"\r\nx = 1\r\n'
    rewritten = core.rewrite_source(source)
    assert rewritten.endswith('("--sql\\nSELECT 1::INTEGER")\r\nx = 1\r\n')
    assert core.strip_source(rewritten) == source


@pytest.fixture()
def many_queries() -> str:
    return "\n".join(
        f'q{i} = "--sql\\nSELECT id::INTEGER FROM t{i}"\nx{i} = {i}' for i in range(50)
    )


@pytest.mark.parametrize(
    ("name", "budget_ms"),
    [("rewrite_source", 500), ("strip_source", 200), ("check_source", 500)],
)
def test_latency(many_queries: str, name: str, budget_ms: int) -> None:
    source = core.rewrite_source(many_queries) if name == "strip_source" else many_queries
    start = time.perf_counter()
    getattr(core, name)(source)
    elapsed_ms = (time.perf_counter() - start) * 1000
    assert elapsed_ms < budget_ms, f"{name}: {elapsed_ms:.0f} ms for 50 queries"


def test_infer_type_latency() -> None:
    start = time.perf_counter()
    for _ in range(100):
        core.infer_type("SELECT id::INTEGER, name::TEXT FROM users")
    assert (time.perf_counter() - start) * 10 < 50, "infer_type: over 50 ms per call"


@pytest.mark.parametrize("name", ["rewrite_source", "strip_source", "fix_source", "check_source"])
def test_unsupported_import_style_raises(name: str) -> None:
    with pytest.raises(ValueError, match="Unsupported import-style: 'from x import SQL'"):
        getattr(core, name)('q = "--sql\\nSELECT 1"', import_style="from x import SQL")


def test_user_type_check_skips_unknown_sum_column() -> None:
    source = (
        "from typing import NamedTuple\nfrom sqlty import SQL\n"
        "class Row(NamedTuple):\n    total: float\n"
        'q = SQL[Row]("--sql\\nSELECT SUM(nope) AS total FROM users")\n'
    )
    assert core.check_source(source, schema_sql="CREATE TABLE users (id INT);") == []
