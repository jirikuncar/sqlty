# Type-mapping comparisons

`harness.py` runs rows of another source (a column type as DDL, or an expression over a
typed table, with the Python type the source expects) through
`sqlty.core.analyze_select`, and writes a report: match, mismatch, sqlty gap (sqlty
leaves it untyped) or not comparable (the source has no Python type).

```bash
uv run --with sqlalchemy==2.0.43 python scripts/compat/run.py sqlalchemy
```

writes `reports/sqlalchemy.json` (every row) and `reports/sqlalchemy.md` (counts and
the rows that differ). Sources run through `uv run --with`, so they are not dependencies
of the project.

A source is a module with a `rows()` generator, registered in `SOURCES` of `run.py`.

An ORM is not the oracle: sqlty types what the reference driver returns (see
`docs/reference/types.md`), so a mismatch is either a sqlty defect, to fix with an
mdtest, or a converter difference, to document.
