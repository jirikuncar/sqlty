# piccolo 1.36.0 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| postgres | column | 26 | 0 | 0 | 0 |
| sqlite | column | 17 | 9 | 0 | 0 |
| **all** | | **43** | **9** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `BOOLEAN` | `bool` | `int` | columns.Boolean |
| sqlite | `DATE` | `datetime.date` | `str` | columns.Date |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | columns.Decimal |
| sqlite | `SECONDS` | `datetime.timedelta` | `float` | columns.Interval |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | columns.Numeric |
| sqlite | `TIME` | `datetime.time` | `str` | columns.Time |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | columns.Timestamp |
| sqlite | `TIMESTAMPTZ` | `datetime.datetime` | `str` | columns.Timestamptz |
| sqlite | `UUID` | `uuid.UUID` | `str` | columns.UUID |
