"""The engine releases the GIL, so threads may call it at once and must agree with one thread."""

import sys
import threading
import uuid
from concurrent.futures import ThreadPoolExecutor

import pytest

from sqlty import _core

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
