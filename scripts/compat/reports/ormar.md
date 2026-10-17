# ormar 0.26.0 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 12 | 2 | 0 | 0 |
| postgres | column | 13 | 1 | 0 | 0 |
| sqlite | column | 8 | 6 | 0 | 0 |
| **all** | | **33** | **9** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `DECIMAL(10, 2)` | `decimal.Decimal` | `float` | ormar.Decimal |
| sqlite | `BOOLEAN` | `bool` | `int` | ormar.Boolean |
| sqlite | `DATE` | `datetime.date` | `str` | ormar.Date |
| sqlite | `DATETIME` | `datetime.datetime` | `str` | ormar.DateTime |
| mysql | `TIME` | `datetime.time` | `datetime.timedelta` | ormar.Time |
| sqlite | `TIME` | `datetime.time` | `str` | ormar.Time |
| postgres | `CHAR(32)` | `uuid.UUID` | `str` | ormar.UUID |
| mysql | `CHAR(32)` | `uuid.UUID` | `str` | ormar.UUID |
| sqlite | `CHAR(32)` | `uuid.UUID` | `str` | ormar.UUID |
