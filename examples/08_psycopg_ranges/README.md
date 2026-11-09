# psycopg Ranges

Runnable project for the [`driver`](../../docs/reference/configuration.md#driver) key. `pyproject.toml` sets `driver = "psycopg"`, so the `tstzrange` and `daterange` columns of `schema.sql` are typed as psycopg's `Range` classes.

```bash
cd examples/08_psycopg_ranges
sqlty check .   # no unknown-type for the range columns
```

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
