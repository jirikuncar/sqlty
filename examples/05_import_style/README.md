# Import Style

Runnable project for [import style](../../docs/reference/configuration.md). `pyproject.toml` sets `import-style`, so queries are wrapped as `Q[T](...)`.

```bash
cd examples/05_import_style
sqlty check .
sqlty strip . && sqlty rewrite .   # round trip, no diff
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
