# User-Defined Row Types

Runnable project for [user-defined row types](../../docs/guide/user-types.md). `queries.py` annotates a query with a `TypedDict` that `sqlty check` validates.

```bash
cd examples/04_check_and_types
sqlty check .
sqlty strip . && sqlty rewrite .   # round trip, no diff
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
