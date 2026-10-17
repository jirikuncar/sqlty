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

## Sources and results

| Source | Run with | Report | sqlty defects found |
|--------|----------|--------|---------------------|
| SQLAlchemy 2.0.43 | `--with sqlalchemy==2.0.43` | `reports/sqlalchemy.md` | 7, fixed in #412 |
| Drivers (live) | `SQLTY_PG_DSN`, `SQLTY_MYSQL_DSN`, `--with psycopg2-binary --with asyncpg --with pymysql --with mysqlclient --with aiosqlite` | `reports/drivers.md` | PostgreSQL geometric types (#411) |
| sqlglot 30.23.0 | a server, as above | `reports/sqlglot.md` | MySQL binary strings, `ROUND`/`TRUNCATE`, functions; PostgreSQL range operators (#413) |
| Django 6.1.2 | `--with django==6.1.2 --with psycopg --with mysqlclient` | `reports/django.md` | none |
| Tortoise 1.1.9 | `--with tortoise-orm==1.1.9` | `reports/tortoise.md` | none |
| SQLModel 0.0.48 | `--with sqlmodel==0.0.48` | `reports/sqlmodel.md` | none |
| Piccolo 1.36.0 | `--with piccolo==1.36.0` | `reports/piccolo.md` | none |
| ormar 0.26.0 | `--with ormar==0.26.0` | `reports/ormar.md` | none |
| peewee 4.5.3 | `--with peewee==4.5.3` | `reports/peewee.md` | none |
| Pony 0.7.20 | a server, `--with pony==0.7.20 --with psycopg2-binary --with pymysql` | `reports/pony.md` | none |
| SQLObject 3.14.0 | `--with sqlobject==3.14.0` | `reports/sqlobject.md` | none |

Every DDL spelling the ORMs write parses and has a type. Their remaining mismatches are
converters above the driver, listed in `docs/guide/drivers-and-orms.md`: SQLite dates,
booleans, decimals and UUIDs; UUIDs stored as `CHAR(32)`, `CHAR(36)` or `BINARY(16)`;
durations stored as `BIGINT` microseconds or `DATETIME`; MySQL `TIME` as `datetime.time`.
