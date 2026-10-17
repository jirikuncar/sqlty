# drivers live vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 22 | 1 | 0 | 0 |
| postgres | column | 36 | 0 | 3 | 2 |
| sqlite | column | 19 | 1 | 0 | 0 |
| **all** | | **77** | **2** | **3** | **2** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `FLOATING POINT` | `float` | `int` | sqlite3: float, aiosqlite: float |
| mysql | `DATE` | `str` | `datetime.date` | pymysql: str, mysqlclient: NoneType |

## sqlty-gap

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| postgres | `INT4RANGE` | `psycopg.types.range.Range` | `None` | psycopg: psycopg.types.range.Range, psycopg2: psycopg2._range.NumericRange, asyncpg: asyncpg.types.Range |
| postgres | `TSTZRANGE` | `psycopg.types.range.Range` | `None` | psycopg: psycopg.types.range.Range, psycopg2: psycopg2._range.DateTimeTZRange, asyncpg: asyncpg.types.Range |
| postgres | `INT4MULTIRANGE` | `psycopg.types.multirange.Multirange` | `None` | psycopg: psycopg.types.multirange.Multirange, psycopg2: str, asyncpg: list |
