"""The engine releases the GIL, so threads may call it at once and must agree with one thread."""

from concurrent.futures import ThreadPoolExecutor

from sqlty import _core

SOURCE = 'q = "--sql\\nSELECT id::INTEGER FROM users"\nr = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")\n'
SCHEMA = "CREATE TABLE users (id INTEGER NOT NULL);"


def _work(_: int) -> tuple[object, ...]:
    return (
        _core.check_source(SOURCE, schema_sql=SCHEMA),
        _core.rewrite_source(SOURCE, schema_sql=SCHEMA),
        _core.fix_source(SOURCE, schema_sql=SCHEMA),
        _core.strip_source(SOURCE),
    )


def test_concurrent_calls_match_a_single_call() -> None:
    expected = _work(0)
    with ThreadPoolExecutor(max_workers=8) as pool:
        assert all(result == expected for result in pool.map(_work, range(200)))
