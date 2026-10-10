# Quick Start

Runnable project for [quick start](../../docs/guide/quick-start.md). `queries.py` holds queries with `::TYPE` casts, already rewritten.

```bash
cd examples/01_quick_start
sqlty check .
sqlty strip . && sqlty rewrite .   # round trip, no diff
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
