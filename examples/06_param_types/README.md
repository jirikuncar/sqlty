# Parameter Types

Runnable project for [typed parameters](../../docs/guide/parameters.md#typed-parameters). `pyproject.toml` sets `param-types = true`, so `sqlty rewrite` writes the parameters of each query, `SQL[R, P]`.

```bash
cd examples/06_param_types
sqlty check .
sqlty strip . && sqlty rewrite .   # round trip, no diff
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
