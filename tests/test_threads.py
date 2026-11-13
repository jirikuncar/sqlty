"""The engine releases the GIL, so threads may call it at once and must agree with one thread."""

import sys
import threading
import uuid
from concurrent.futures import ThreadPoolExecutor

import pytest

from sqlty import _core, core

SOURCE = 'q = "--sql\\nSELECT id::INTEGER FROM users"\nr = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")\n'
SCHEMA = "CREATE TABLE users (id INTEGER NOT NULL);"
COLUMN_TYPES = ["INTEGER", "TEXT", "BIGINT", "BOOLEAN", "DATE", "UUID"]

# `sys._is_gil_enabled` exists from 3.13; on a free-threaded build it returns False and
# threads really run the engine in parallel.
FREE_THREADED = not getattr(sys, "_is_gil_enabled", lambda: True)()


# The result type of this query follows the schema's column type.
UNCAST = 'q = "--sql\\nSELECT id FROM users"\n'


def _work(schema: str) -> tuple[object, ...]:
    return (
        _core.check_source(SOURCE, schema_sql=schema),
        _core.rewrite_source(SOURCE, schema_sql=schema),
        _core.rewrite_source(UNCAST, schema_sql=schema),
        _core.fix_source(SOURCE, schema_sql=schema),
        _core.strip_source(SOURCE),
    )


def test_concurrent_calls_match_a_single_call() -> None:
    expected = _work(SCHEMA)
    with ThreadPoolExecutor(max_workers=8) as pool:
        assert all(result == expected for result in pool.map(_work, [SCHEMA] * 200))


def test_cold_schema_cache_misses_race_to_the_same_answers() -> None:
    """Every thread starts on schemas the cache has never seen, all at the same moment.

    Several threads miss together on one schema, parse it unlocked and race to insert it;
    the answers must still equal a sequential run of the same schemas. The race is only
    real when threads run in parallel, i.e. on a free-threaded build; on a GIL build this
    still checks correctness but cannot expose the race.
    """
    unique = uuid.uuid4().hex
    schemas = [
        f"CREATE TABLE users (id {ctype} NOT NULL); CREATE TABLE cold_{unique}_{n} (x INT);"
        for n, ctype in enumerate(COLUMN_TYPES)
    ]
    workers = 4 * len(schemas)
    barrier = threading.Barrier(workers)

    def cold(n: int) -> tuple[int, tuple[object, ...]]:
        barrier.wait()
        return n % len(schemas), _work(schemas[n % len(schemas)])

    with ThreadPoolExecutor(max_workers=workers) as pool:
        concurrent = list(pool.map(cold, range(workers)))

    # Computed afterwards, single-threaded, so it does not warm the cache beforehand.
    expected = [_work(schema) for schema in schemas]
    assert len({repr(result) for result in expected}) > 1, "schemas must give different answers"
    for index, result in concurrent:
        assert result == expected[index]


@pytest.mark.skipif(not FREE_THREADED, reason="the cache-miss race needs parallel threads")
def test_free_threaded_build_runs_threads_in_parallel() -> None:
    assert not sys._is_gil_enabled()


def _chain(terms: int) -> str:
    return "SELECT " + " + ".join(["id"] * terms) + " AS total FROM users"


def test_deep_expressions_analyze_on_a_small_thread_stack() -> None:
    result: list[object] = []
    previous = threading.stack_size(512 * 1024)
    try:
        thread = threading.Thread(
            target=lambda: result.append(core.infer_type(_chain(999), schema_sql=SCHEMA))
        )
        thread.start()
        thread.join()
    finally:
        threading.stack_size(previous)
    assert result == ["tuple[int]"]


def test_expressions_nested_too_deep_are_not_analyzed() -> None:
    sql = _chain(1001)
    assert core.analyze_select(sql, schema_sql=SCHEMA) is None
    source = f'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\n{sql}")\n'
    assert [(d["rule"], d["message"]) for d in core.check_source(source, schema_sql=SCHEMA)] == [
        ("unparsed-query", "query does not parse: expressions nest more than 1000 levels deep")
    ]


MODULES = {
    "pkg.a": "from sqlty import SQL\nfrom pkg.rows import R\n"
    'q = SQL[R]("--sql\\nSELECT id, id AS x FROM users")  # sqlty: ignore[narrow-type]\n',
    "pkg.rows": "from typing import TypedDict\nclass R(TypedDict):\n    id: int\n    x: int | None\n",
    "pkg.b": 'from sqlty import SQL\nq = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")  # sqlty: ignore\n'
    'r = SQL[tuple[int]]("--sql\\nSELECT id FROM users")  # sqlty: ignore\n',
    "pkg.deep": f'q = "--sql\\n{_chain(999)}"\n',
    "pkg.broken": 'q = "--sql\\nSELECT nope FROM users"  # sqlty: ignore[unknown-column]\ndef (\n',
}


@pytest.mark.parametrize("jobs", [1, 3])
def test_check_modules_reports_what_check_source_does(jobs: int) -> None:
    from sqlty._check import RowUsage, check_modules
    from sqlty._helpers import HelperIndex
    from sqlty._source import ParsedSource

    index = HelperIndex()
    parsed = {name: ParsedSource(text, name) for name, text in MODULES.items()}
    for name, source in parsed.items():
        index.add(name, source)
    schema = core.load_schema(SCHEMA)
    configs = [
        (
            "postgres",
            schema,
            {"redundant-optional": "error"},
            None,
            False,
            frozenset(),
            index,
            None,
        ),
        ("sqlite", schema, {"unused-ignore": "ignore"}, None, True, frozenset(), None, 4),
    ]
    modules = [(source, configs[n % 2]) for n, source in enumerate(parsed.values())]
    usage, expected_usage = RowUsage(), RowUsage()

    found = check_modules(modules, usage, jobs)

    expected = [
        core.check_source(
            source,
            dialect=dialect,
            schema_sql=schema,
            rules=rules,
            untyped=untyped,
            helpers=helpers,
            row_usage=expected_usage,
            query_variants=variants,
        )
        for source, (dialect, schema, rules, _, untyped, _, helpers, variants) in modules
    ]
    assert found == expected
    assert any(found)
    flat = [diag for diagnostics in found for diag in diagnostics]
    assert usage.filter(flat) == expected_usage.filter(
        [diag for diagnostics in expected for diag in diagnostics]
    )
