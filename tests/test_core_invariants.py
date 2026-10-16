"""Properties of ``sqlty.core`` that hold for every input.

Values are specified in Markdown (``tests/mdtest``, ``docs``); this module
checks invariants over all of that code, and what Markdown can't express
(exceptions, CRLF, timing).
"""

from __future__ import annotations

import ast
import random
import time
from pathlib import Path
from typing import Any

import pytest

import sqlty.core as core
from sqlty._source import ParsedSource
from tests._mdtest.parser import parse
from tests._mdtest.runner import comments, string_constants, without_generated

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
                "schema_sql": section.schema_sql,
                "import_style": settings.get("import-style"),
                "untyped": settings.get("untyped") == "sentinel",
                "json_types": settings.get("json-types", "typing.Any"),
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


@pytest.mark.parametrize("eol", ["\n", "\r\n"], ids=["lf", "crlf"])
@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_strip_undoes_rewrite(source: str, options: dict, eol: str) -> None:
    """``strip(rewrite(x)) == x`` for sources that ``strip`` leaves alone.

    Other sqlty imports may be there: rewrite adds what is missing on lines of
    its own, which strip removes again. Strip also removes the lines it would
    add when they were written by hand, so those sources are skipped.
    """
    style = options["import_style"]
    own_lines = {style or "from sqlty import SQL", "from sqlty import UNTYPED"}
    if (
        own_lines & {line.strip() for line in source.splitlines()}
        or core.strip_source(source, import_style=style) != source
    ):
        pytest.skip("source already uses sqlty")
    source = source.replace("\n", eol)
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
    """``rewrite``, ``strip`` and ``check --fix`` keep valid Python valid and every
    comment, rewrite and strip keep every string as it is, and ``check --fix`` is idempotent."""
    original = without_generated(source)
    expected, kept = string_constants(original), comments(original)
    rewritten = without_generated(core.rewrite_source(source, **options))
    assert (string_constants(rewritten), comments(rewritten)) == (expected, kept), rewritten
    stripped = core.strip_source(source, import_style=options["import_style"])
    assert (string_constants(stripped), comments(stripped)) == (expected, kept), stripped
    fixed = core.fix_source(source, **options)
    ast.parse(fixed)
    assert comments(without_generated(fixed)) == kept, fixed
    assert core.fix_source(fixed, **options) == fixed


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_outputs_parse(source: str, options: dict) -> None:
    _check_outputs(source, options)


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_crlf_outputs(source: str, options: dict) -> None:
    """With CRLF line endings, every command gives the LF result with CRLF line endings."""
    source = source if source.endswith("\n") else source + "\n"
    crlf = source.replace("\n", "\r\n")
    _check_outputs(crlf, options)
    to_crlf = lambda text: text.replace("\n", "\r\n")  # noqa: E731
    style = options["import_style"]
    assert core.rewrite_source(crlf, **options) == to_crlf(core.rewrite_source(source, **options))
    assert core.strip_source(crlf, import_style=style) == to_crlf(
        core.strip_source(source, import_style=style)
    )
    assert core.fix_source(crlf, **options) == to_crlf(core.fix_source(source, **options))


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
    'n = f"""{x:\n}"""\n',
    'o = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER",  # c\n)\n',
    'p = mod.SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")\n',
    '"""Example: SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER AS n")"""\n',
    'r = (a\n@ b, "--sql\\nSELECT 1::INTEGER "\n"AS n")\n',
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


@pytest.mark.parametrize("newline", ["\n", "\r\n"])
def test_fix_suppression_follows_inserted_import(newline: str) -> None:
    source = (
        '"""Doc."""\nr = "--sql\\nSELECT id FROM t"\n'
        'q = "--sql\\nSELECT * FROM t"  # sqlty: ignore[select-star]\n'
    ).replace("\n", newline)
    fixed = core.fix_source(source, schema_sql="CREATE TABLE t (id INT);")
    assert fixed.count("from sqlty import SQL") == 1
    assert f'SELECT * FROM t")  # sqlty: ignore[select-star]{newline}' in fixed


@pytest.mark.parametrize(("source", "options"), PYTHON)
def test_skipping_the_rust_checker_changes_no_diagnostics(
    source: str, options: dict, monkeypatch: pytest.MonkeyPatch
) -> None:
    """``check_source`` skips the engine for a file without SQL markers; nothing is lost.

    Checks the code as written, rewritten, stripped and fixed, so files with and
    without markers, wrappers and ``UNTYPED`` are all compared.
    """
    style = options["import_style"]
    variants = [
        source,
        core.rewrite_source(source, **options),
        core.strip_source(source, import_style=style),
        core.fix_source(source, **options),
    ]
    checked = {key: value for key, value in options.items() if key != "json_types"}
    with_shortcut = [core.check_source(text, **checked) for text in variants]
    monkeypatch.setattr(ParsedSource, "may_need_rust_check", lambda self, import_style=None: True)
    assert with_shortcut == [core.check_source(text, **checked) for text in variants]


@pytest.mark.parametrize(
    "source",
    [
        'q = "--" "sql\\nSELECT 1"\n',
        'q = ("-"\n    "-sql\\nSELECT 1")\n',
        'q = ("--s"  # why\n    "ql\\nSELECT 1")\n',
    ],
)
def test_split_marker_is_not_skipped(source: str) -> None:
    assert ParsedSource(source).may_need_rust_check()
    assert [d["rule"] for d in core.check_source(source)] == ["stale-rewrite"]


def test_unsupported_import_style_still_raises_without_markers() -> None:
    with pytest.raises(ValueError):
        core.check_source("x = 1\n", import_style="import nothing")
