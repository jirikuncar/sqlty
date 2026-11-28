# rustmy

Runnable project for [rustmy](../../docs/guide/rustmy.md), the MySQL driver written in Rust. The queries in `queries.py` run through `sqlty.drivers.rustmy`, so the type checker sees `User` rows and rustmy builds them, and through a plain rustmy cursor, whose parameters `sqlty check` counts.

```bash
cd examples/09_rustmy
sqlty check .   # rows match schema.sql, every call passes its query's parameters
```

Break `rename` to see a report: drop `user_id` from its parameters, and `sqlty check` reports `param-count-mismatch`.

CI keeps it current: `tests/test_docs.py` fails if a query needs rewriting
or a row type is stale.
