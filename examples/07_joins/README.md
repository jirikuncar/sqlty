# Joins and Aggregates

Runnable project for [joins and aggregates](../../docs/guide/joins.md). `schema.sql` declares the keys sqlty reads to find totals a join inflates and joins on the wrong key; `pyproject.toml` turns on the opt-in rules.

```bash
cd examples/07_joins
sqlty check .   # fanout-aggregate, chasm-join and entity-mismatch warnings
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
