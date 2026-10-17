# tortoise 1.1.9 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 16 | 4 | 0 | 0 |
| postgres | column | 18 | 2 | 0 | 0 |
| sqlite | column | 11 | 9 | 0 | 0 |
| **all** | | **45** | **15** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `INT` | `bool` | `int` | fields.BooleanField |
| sqlite | `DATE` | `datetime.date` | `str` | fields.DateField |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | fields.DateTimeField |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | fields.DatetimeField |
| sqlite | `VARCHAR(40)` | `decimal.Decimal` | `str` | fields.DecimalField |
| postgres | `BIGINT` | `datetime.timedelta` | `int` | fields.DurationField |
| mysql | `BIGINT` | `datetime.timedelta` | `int` | fields.DurationField |
| sqlite | `BIGINT` | `datetime.timedelta` | `int` | fields.DurationField |
| postgres | `BIGINT` | `datetime.timedelta` | `int` | fields.TimeDeltaField |
| mysql | `BIGINT` | `datetime.timedelta` | `int` | fields.TimeDeltaField |
| sqlite | `BIGINT` | `datetime.timedelta` | `int` | fields.TimeDeltaField |
| mysql | `TIME(6)` | `datetime.time` | `datetime.timedelta` | fields.TimeField |
| sqlite | `TIME` | `datetime.time` | `str` | fields.TimeField |
| mysql | `CHAR(36)` | `uuid.UUID` | `str` | fields.UUIDField |
| sqlite | `CHAR(36)` | `uuid.UUID` | `str` | fields.UUIDField |
