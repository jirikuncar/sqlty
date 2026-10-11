"""Properties of ``sqlty.core`` that hold for every input.

Values are specified in Markdown (``tests/mdtest``, ``docs``); this module
checks invariants over all of that code, and what Markdown can't express
(exceptions, CRLF, timing).
"""

from __future__ import annotations

import ast
import random
import re
import time
from pathlib import Path
from typing import Any

import pytest

import sqlty.core as core
from tests._mdtest.parser import parse
from tests._mdtest.runner import string_constants

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


def _check_outputs(source: str, options: dict) -> None:
    """``rewrite``, ``strip`` and ``check --fix`` keep valid Python valid, rewrite and
    strip keep every string as it is, and ``check --fix`` is idempotent."""
    expected = string_constants(source)
    rewritten = core.rewrite_source(source, **options)
    assert string_constants(rewritten) == expected, rewritten
    stripped = core.strip_source(source, import_style=options["import_style"])
    assert string_constants(stripped) == expected, stripped
    fixed = core.fix_source(source, **options)
    ast.parse(fixed)
    assert core.fix_source(fixed, **options) == fixed


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_outputs_parse(source: str, options: dict) -> None:
    _check_outputs(source, options)


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_crlf_outputs(source: str, options: dict) -> None:
    """With CRLF line endings, every command gives the LF result with CRLF line endings."""
    crlf = source.replace("\n", "\r\n")
    _check_outputs(crlf, options)
    lf = lambda text: text.replace("\r\n", "\n")  # noqa: E731
    style = options["import_style"]
    assert lf(core.rewrite_source(crlf, **options)) == core.rewrite_source(source, **options)
    assert lf(core.strip_source(crlf, import_style=style)) == core.strip_source(
        source, import_style=style
    )
    assert lf(core.fix_source(crlf, **options)) == core.fix_source(source, **options)


_FRAGMENTS = [
    'x = f"{"("}"\n',
    'y = 1 if"(" else 2\n',
    'def f():\n    return"("\n',
    '"""Doc."""\n',
    "# it's a (comment\n",
    'a = "--sql\\nSELECT 1::INTEGER AS n"\n',
    "b = ('--sql\\nSELECT 1::INTEGER '  # c\n     'AS n')\n",
    'c = "--sql\\nSELECT 1::INTEGER " \\\r\n    "AS n"\n',
    "d = SQL[tuple[str]](\n    '--sql\\nSELECT 1::INTEGER '\n    'AS n'\n)\n",
    'e = SQL[tuple[str]]("""--sql\nSELECT 1::INTEGER AS n"""  # c\n)\n',
    'g = SQL[tuple[int]]("--sql\\nSELECT 1 AS n -- \\"(\\"")\n',
    'h = SQL[T]("--sql\\nSELECT 1" + x), mod.SQL[T]("--sql\\nSELECT 1")\n',
    'i = (f"--sql\\nSELECT {x} "\n     "AS n")\n',
    "j = r'--sql\\nSELECT 1::INTEGER'\n",
    "k = [\n    SQL[tuple[int]]('--sql\\nSELECT 1::INTEGER', ),\n]\n",
    'l = t"{x:{"("}}"\n',
    "m = '--sql\\nVALUES (1, 2)'\n",
]


def test_fuzz_fragments() -> None:
    """Random sequences of tricky fragments: outputs stay valid and keep their strings."""
    rng = random.Random(127)
    checked = 0
    for _ in range(1500):
        source = "".join(rng.choices(_FRAGMENTS, k=rng.randint(1, 5)))
        try:
            ast.parse(source)
        except SyntaxError:
            continue
        options = {
            "dialect": "postgres",
            "schema_sql": None,
            "import_style": None,
            "untyped": rng.random() < 0.5,
        }
        _check_outputs(source, options)
        checked += 1
    assert checked > 1000


def test_unknown_schema_column_raises() -> None:
    with pytest.raises(RuntimeError, match="Column 'nope' not found in table 'users'"):
        core.infer_type("SELECT SUM(nope) FROM users", schema_sql="CREATE TABLE users (id INT);")


def test_crlf_continuation() -> None:
    source = 'q = "--sql\\nSELECT 1::INTEGER " \\\r\n    "AS n"\r\n'
    for output in (core.rewrite_source(source), core.fix_source(source)):
        assert 'SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER " \\\r\n    "AS n")' in output
        ast.parse(output)


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
