# sqlmodel 0.0.48 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 8 | 3 | 0 | 0 |
| postgres | column | 11 | 0 | 0 | 0 |
| sqlite | column | 4 | 7 | 0 | 0 |
| **all** | | **23** | **10** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `BOOLEAN` | `bool` | `int` | annotation bool |
| sqlite | `NUMERIC(10, 2)` | `decimal.Decimal` | `float` | annotation decimal.Decimal |
| sqlite | `DATETIME` | `datetime.datetime` | `str` | annotation datetime.datetime |
| sqlite | `DATE` | `datetime.date` | `str` | annotation datetime.date |
| mysql | `TIME` | `datetime.time` | `datetime.timedelta` | annotation datetime.time |
| sqlite | `TIME` | `datetime.time` | `str` | annotation datetime.time |
| mysql | `DATETIME` | `datetime.timedelta` | `datetime.datetime` | annotation datetime.timedelta |
| sqlite | `DATETIME` | `datetime.timedelta` | `str` | annotation datetime.timedelta |
| mysql | `CHAR(32)` | `uuid.UUID` | `str` | annotation uuid.UUID |
| sqlite | `CHAR(32)` | `uuid.UUID` | `str` | annotation uuid.UUID |
