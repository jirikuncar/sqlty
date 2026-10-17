# pony 0.7.20 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 10 | 2 | 0 | 0 |
| postgres | column | 12 | 0 | 0 | 0 |
| sqlite | column | 6 | 6 | 0 | 0 |
| **all** | | **28** | **8** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `BOOLEAN` | `bool` | `int` | attribute bool |
| sqlite | `DECIMAL(12, 2)` | `decimal.Decimal` | `float` | attribute decimal.Decimal |
| sqlite | `DATETIME` | `datetime.datetime` | `str` | attribute datetime.datetime |
| sqlite | `DATE` | `datetime.date` | `str` | attribute datetime.date |
| sqlite | `TIME` | `datetime.time` | `str` | attribute datetime.time |
| sqlite | `UUID` | `uuid.UUID` | `str` | attribute uuid.UUID |
| mysql | `TIME` | `datetime.time` | `datetime.timedelta` | attribute datetime.time |
| mysql | `BINARY(16)` | `uuid.UUID` | `bytes` | attribute uuid.UUID |
