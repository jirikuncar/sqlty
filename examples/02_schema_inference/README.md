# Schema Inference

Runnable project for [schema inference](../../docs/guide/schema-inference.md). `pyproject.toml` points at `schema.sql`, so `queries.py` needs no casts.

```bash
cd examples/02_schema_inference
sqlty check .
sqlty strip . && sqlty rewrite .   # round trip, no diff
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
