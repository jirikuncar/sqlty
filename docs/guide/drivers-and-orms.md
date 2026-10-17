# Drivers and ORMs

sqlty types the value your database driver returns. When you run a query through
an ORM, the ORM may convert that value first, and its Python type can differ.
This page lists where, and what to annotate.

The comparisons behind it are in `scripts/compat/`: each ORM's column types and
expressions, and what each driver returns from live servers.

## The reference drivers

| Dialect | sqlty follows | Other drivers |
|---------|---------------|---------------|
| `postgres` | psycopg 3 | asyncpg returns `json`/`jsonb` as `str`, `bit` as `asyncpg.BitString`, ranges as `asyncpg.Range`; psycopg2 returns `bytea` as `memoryview`, and `uuid` and `inet` as `str` unless you register their adapters |
| `mysql` | PyMySQL | mysqlclient agrees, except that it returns a zero date as `None` where PyMySQL returns `str` |
| `sqlite` | `sqlite3`, without `detect_types` | aiosqlite agrees |

The full table, with the reason for each type, is [Type mapping](../reference/types.md).

## Where ORMs convert

| Value | Driver returns (sqlty) | ORMs return |
|-------|------------------------|-------------|
| SQLite `DATE`, `DATETIME`, `TIME` | `str` | `datetime.date`, `datetime.datetime`, `datetime.time` (all of them) |
| SQLite `BOOLEAN` | `int` | `bool` |
| SQLite `NUMERIC`, `DECIMAL` | `float` | `decimal.Decimal` (SQLAlchemy, Django, SQLModel, Pony, peewee) |
| A UUID stored as `CHAR(32)`, `CHAR(36)`, `VARCHAR(40)` or `BINARY(16)` | `str` or `bytes` | `uuid.UUID` |
| A duration stored as `BIGINT` microseconds (Django, Tortoise) or `DATETIME` (SQLAlchemy on MySQL and SQLite) | `int`, `datetime.datetime` | `datetime.timedelta` |
| MySQL `TIME` | `datetime.timedelta` | `datetime.time` (SQLAlchemy, Tortoise, ormar, peewee, Pony) |
| `JSON` in MySQL and SQLite | `str`, typed `typing.Any` | the decoded value |
| `AVG` of integers in PostgreSQL | `decimal.Decimal` | `float` (Django's `Avg`) |

## What to annotate

Annotate what reaches your code. If you run the query through the driver, as
`SQL[T]` and `sqlty.drivers.psycopg` do, that is the driver's type, and sqlty
checks it:

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE events (id INTEGER PRIMARY KEY, day DATE NOT NULL, done BOOLEAN NOT NULL);
```

```python
from typing import NamedTuple

from sqlty import SQL


class Event(NamedTuple):
    id: int
    day: str
    done: int


# error:
events = SQL[Event]("--sql\nSELECT id, day, done FROM events")
```

If an ORM converts the values before your code sees them, annotate the
converted type and tell sqlty the column is converted on purpose:

```python
import datetime
from typing import NamedTuple

from sqlty import SQL


class Event(NamedTuple):
    id: int
    day: datetime.date
    done: bool


# error: [column-type-mismatch] "Event.day is datetime.date but query column 'day' infers str"
# error: [column-type-mismatch] "Event.done is bool but query column 'done' infers int"
events = SQL[Event]("--sql\nSELECT id, day, done FROM events")
converted = SQL[Event]("--sql\nSELECT id, day, done FROM events")  # sqlty: ignore[column-type-mismatch]
```

The ignore comment drops the whole row's type check, so keep it to queries an ORM
really runs. SQLite has no date or boolean type to cast to, so a cast does not
help there.

## Expressions ORMs type differently

Some ORMs give an expression a type the database does not return. sqlty follows
the database:

| Expression | Database returns | ORM says |
|------------|------------------|----------|
| `percent_rank()`, `cume_dist()` | `double precision`: `float` | `Decimal` (SQLAlchemy) |
| `int || text` | `text`: `str` | `int` (SQLAlchemy) |
| `int % numeric` | `numeric`: `decimal.Decimal` | `int` (SQLAlchemy) |
| `LOCALTIME` | `time` | `datetime` (SQLAlchemy) |
| MySQL `SUM(int)`, `int / int` | `DECIMAL`: `decimal.Decimal` | `int` (SQLAlchemy, Django) |
