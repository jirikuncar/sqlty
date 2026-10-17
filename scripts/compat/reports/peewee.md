# peewee 4.5.3 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 16 | 5 | 0 | 0 |
| postgres | column | 18 | 3 | 0 | 0 |
| sqlite | column | 12 | 9 | 0 | 0 |
| **all** | | **46** | **17** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `DECIMAL(10, 5)` | `decimal.Decimal` | `float` | peewee.DecimalField |
| mysql | `VARCHAR(40)` | `uuid.UUID` | `str` | peewee.UUIDField |
| sqlite | `TEXT` | `uuid.UUID` | `str` | peewee.UUIDField |
| postgres | `BYTEA` | `uuid.UUID` | `bytes` | peewee.BinaryUUIDField |
| mysql | `VARBINARY(16)` | `uuid.UUID` | `bytes` | peewee.BinaryUUIDField |
| sqlite | `UUIDB` | `uuid.UUID` | `float` | peewee.BinaryUUIDField |
| sqlite | `DATETIME` | `datetime.datetime` | `str` | peewee.DateTimeField |
| sqlite | `DATE` | `datetime.date` | `str` | peewee.DateField |
| mysql | `TIME` | `datetime.time` | `datetime.timedelta` | peewee.TimeField |
| sqlite | `TIME` | `datetime.time` | `str` | peewee.TimeField |
| postgres | `BIGINT` | `datetime.datetime` | `int` | peewee.TimestampField |
| mysql | `BIGINT` | `datetime.datetime` | `int` | peewee.TimestampField |
| sqlite | `INTEGER` | `datetime.datetime` | `int` | peewee.TimestampField |
| sqlite | `INTEGER` | `bool` | `int` | peewee.BooleanField |
| postgres | `BIGINT` | `str` | `int` | peewee.IPField |
| mysql | `BIGINT` | `str` | `int` | peewee.IPField |
| sqlite | `INTEGER` | `str` | `int` | peewee.IPField |
