# sqlobject 3.14.0 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 14 | 2 | 0 | 0 |
| postgres | column | 20 | 1 | 0 | 0 |
| sqlite | column | 9 | 10 | 0 | 0 |
| **all** | | **43** | **13** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `TEXT` | `bytes` | `str` | col.BLOBCol |
| sqlite | `BOOLEAN` | `bool` | `int` | col.BoolCol |
| sqlite | `DECIMAL(10, 2)` | `decimal.Decimal` | `float` | col.CurrencyCol |
| sqlite | `DATE` | `datetime.date` | `str` | col.DateCol |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | col.DateTimeCol |
| sqlite | `DECIMAL(10, 2)` | `decimal.Decimal` | `float` | col.DecimalCol |
| postgres | `VARCHAR(12)` | `decimal.Decimal` | `str` | col.DecimalStringCol |
| mysql | `VARCHAR(12)` | `decimal.Decimal` | `str` | col.DecimalStringCol |
| sqlite | `VARCHAR(12)` | `decimal.Decimal` | `str` | col.DecimalStringCol |
| sqlite | `TIME` | `datetime.time` | `str` | col.TimeCol |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | col.TimestampCol |
| mysql | `VARCHAR(36)` | `uuid.UUID` | `str` | col.UuidCol |
| sqlite | `VARCHAR(36)` | `uuid.UUID` | `str` | col.UuidCol |
