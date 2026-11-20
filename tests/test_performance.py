"""Guards against checking or rewriting time growing with strings x file size."""

import time

import pytest

from sqlty import _core, core

pytestmark = pytest.mark.timing

STRINGS = 20_000
BOUND_SECONDS = 0.5


def _source() -> str:
    return "".join(f"x{i} = 'abc'\n" for i in range(STRINGS))


def _best_of(fn, source: str, runs: int = 3) -> float:
    best = float("inf")
    for _ in range(runs):
        start = time.perf_counter()
        fn(source)
        best = min(best, time.perf_counter() - start)
    return best


def test_check_source_scales_linearly() -> None:
    assert _best_of(_core.check_source, _source()) < BOUND_SECONDS


def test_rewrite_source_scales_linearly() -> None:
    assert _best_of(_core.rewrite_source, _source()) < BOUND_SECONDS


def _queries(count: int) -> str:
    return "".join(
        f'q{i} = SQL[tuple[int]]("--sql\\nSELECT {i}::INTEGER")\nr{i} = "--sql\\nSELECT {i}::INTEGER AS x"\n'
        for i in range(count)
    )


def test_wrapped_and_unwrapped_queries_scale_linearly() -> None:
    # 8x the queries must cost well under the 64x a per-string scan of the calls would
    small, large = _queries(500), _queries(4000)
    for fn in (_core.check_source, _core.rewrite_source, _core.query_sites):
        assert _best_of(fn, large) < 24 * _best_of(fn, small) + 0.005, fn.__name__


DEPTH = 20
NESTED = {
    "mysql a + b": ("mysql", " + ".join(["id"] * DEPTH)),
    "minus": ("postgres", "-(" * DEPTH + "id" + ")" * DEPTH),
    "abs": ("postgres", "abs(" * DEPTH + "id" + ")" * DEPTH),
    "or": ("postgres", " OR ".join(["id = 1"] * DEPTH)),
    "sqlite ceil": ("sqlite", "ceil(" * DEPTH + "id" + ")" * DEPTH),
}


@pytest.mark.parametrize(("dialect", "expr"), NESTED.values(), ids=NESTED.keys())
def test_nested_expressions_scale_linearly(dialect: str, expr: str) -> None:
    sql = f"SELECT {expr} AS x FROM users"
    start = time.perf_counter()
    core.analyze_select(
        sql, dialect=dialect, schema_sql="CREATE TABLE users (id INTEGER NOT NULL);"
    )
    assert time.perf_counter() - start < BOUND_SECONDS


def _check_seconds(root, monkeypatch, count: int, key: str, files: dict[str, str]) -> float:
    project = root / f"{key}{count}"
    project.mkdir()
    (project / "pyproject.toml").write_text(
        f'[tool.sqlty]\ndialect = "postgres"\n{key} = "{files["dir"]}/"\n'
    )
    (project / "app").mkdir()
    (project / "app" / "q.py").write_text('q = """--sql\nSELECT 1 AS x"""\n')
    (project / files["dir"]).mkdir()
    for i in range(count):
        (project / files["dir"] / f"{i:05}.sql").write_text(files["text"].format(i=i))
    monkeypatch.chdir(project)
    best = float("inf")
    for _ in range(2):
        start = time.perf_counter()
        assert _core.cli(["sqlty", "check", "-q", "."]) == 0
        best = min(best, time.perf_counter() - start)
    return best


@pytest.mark.parametrize(
    ("key", "files"),
    [
        ("schema", {"dir": "migrations", "text": "CREATE TABLE t{i} (id integer);\n"}),
        ("queries", {"dir": "sql", "text": "-- name: q{i}\nSELECT 1 AS x;\n"}),
    ],
)
def test_many_schema_or_query_files_scale_linearly(tmp_path, monkeypatch, key, files) -> None:
    # 8x the files must cost well under the 64x a pairwise comparison of the paths would
    small = _check_seconds(tmp_path, monkeypatch, 1000, key, files)
    large = _check_seconds(tmp_path, monkeypatch, 8000, key, files)
    assert large < 12 * small + 0.05
