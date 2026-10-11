# Configuration

Runnable project for [configuration](../../docs/reference/configuration.md). `pyproject.toml` sets the dialect, the schema and rule severities.

```bash
cd examples/03_pyproject_config
sqlty check .
sqlty strip . && sqlty rewrite .   # round trip, no diff
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
