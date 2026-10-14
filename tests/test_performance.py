"""Guards against checking or rewriting time growing with strings x file size."""

import time

from sqlty import _core

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
