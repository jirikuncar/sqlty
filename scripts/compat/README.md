# Type-mapping comparisons

`harness.py` runs rows of another source (a column type as DDL, or an expression over a
typed table, with the Python type the source expects) through
`sqlty.core.analyze_select`, and writes a report: match, mismatch, sqlty gap (sqlty
leaves it untyped) or not comparable (the source has no Python type).

```bash
scripts/compat/run.sh sqlalchemy
```

writes `reports/sqlalchemy.json` (every row) and `reports/sqlalchemy.md` (counts and
the rows that differ). With no argument, `run.sh` runs every source. Each source is a
dependency group of `pyproject.toml` here, pinned, and runs through
`uv run --with-requirements`, so none is a dependency of sqlty.

Dependabot bumps the pins weekly. Its pull request runs the `Compatibility` workflow,
which reruns every source against PostgreSQL 17 and MySQL 8.4 and shows how the reports
changed in the job summary; the workflow also runs weekly, for engine changes. Commit
the regenerated reports with a bump.

A source is a module with a `rows()` generator, registered in `SOURCES` of `run.py`.

An ORM is not the oracle: sqlty types what the reference driver returns (see
`docs/reference/types.md`), so a mismatch is either a sqlty defect, to fix with an
mdtest, or a converter difference, to document.

## Sources and results

| Source | Group | Report | sqlty defects found |
|--------|-------|--------|---------------------|
| SQLAlchemy | `sqlalchemy` | `reports/sqlalchemy.md` | 7, fixed in #412 |
| Drivers (live) | `drivers`, a server | `reports/drivers.md` | PostgreSQL geometric types (#411) |
| sqlglot | `sqlglot`, a server | `reports/sqlglot.md` | MySQL binary strings, `ROUND`/`TRUNCATE`, functions; PostgreSQL range operators (#413) |
| Django | `django` | `reports/django.md` | none |
| Tortoise | `tortoise` | `reports/tortoise.md` | none |
| SQLModel | `sqlmodel` | `reports/sqlmodel.md` | none |
| Piccolo | `piccolo` | `reports/piccolo.md` | none |
| ormar | `ormar` | `reports/ormar.md` | none |
| peewee | `peewee` | `reports/peewee.md` | none |
| Pony | `pony`, a server | `reports/pony.md` | none |
| SQLObject | `sqlobject` | `reports/sqlobject.md` | none |
| PostgreSQL nullability | `pg_nullability.py`, a server | `reports/pg_nullability.md` | none |
| MySQL nullability | `mysql_nullability.py`, a server | `reports/mysql_nullability.md` | math functions outside their domain (#587), date arithmetic and casts (#590) |
| PostgreSQL documentation | `pg_docs.py`, a server | `reports/pg_docs.md` | `pg_typeof`, `cbrt`, `length` of a path, text search operators, `ts_headline` after a configuration, `string_agg` of `bytea`, `age` of a transaction ID, session names, arrays of arrays (#558) |

`pg_nullability.py` is not a harness source: it runs the PostgreSQL queries of the Markdown tests
on seeded rows (`tests/_pg_seed.py`) and lists the columns sqlty types as nullable that no
row has NULL in. `tests/test_pg_describe.py` fails on the reverse, a NULL in a column sqlty
types as non-null.

`mysql_nullability.py` is not a harness source either: it runs the MySQL queries of the
Markdown tests on MySQL and compares sqlty's nullability with the one MySQL derives for
`CREATE TABLE ... AS` the query. MySQL marks many expressions nullable that cannot be
NULL, so the report is a list of leads, not of defects. `tests/test_server_describe.py`
fails on a name or type that differs.

`pg_docs.py` reads the examples of the function and operator tables of the PostgreSQL
documentation, for the server's major version, and compares the type the server describes
for each with sqlty's. It does the same for every overload of a documented function in
`pg_proc`, called with a typed `NULL` per argument, and for every operator of
`pg_operator`. It exits with status 1 on a type that differs, and lists the queries sqlty
leaves untyped or can't parse. Run it against a new PostgreSQL release to
find what sqlty doesn't know yet.

Every DDL spelling the ORMs write parses and has a type. Their remaining mismatches are
converters above the driver, listed in `docs/guide/drivers-and-orms.md`: SQLite dates,
booleans, decimals and UUIDs; UUIDs stored as `CHAR(32)`, `CHAR(36)` or `BINARY(16)`;
durations stored as `BIGINT` microseconds or `DATETIME`; MySQL `TIME` as `datetime.time`.
A project reading values through one of them sets `[tool.sqlty.types]`.
