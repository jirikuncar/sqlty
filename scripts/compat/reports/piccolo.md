# piccolo 1.36.0 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| postgres | column | 48 | 0 | 0 | 0 |
| sqlite | column | 17 | 31 | 0 | 0 |
| **all** | | **65** | **31** | **0** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `ARRAY` | `list[int]` | `str` | columns.Array(BigInt) |
| sqlite | `ARRAY` | `list[bytes]` | `str` | columns.Array(Blob) |
| sqlite | `BOOLEAN` | `bool` | `int` | columns.Boolean |
| sqlite | `ARRAY` | `list[bool]` | `str` | columns.Array(Boolean) |
| sqlite | `ARRAY` | `list[bytes]` | `str` | columns.Array(Bytea) |
| sqlite | `ARRAY` | `list[str]` | `str` | columns.Array(Char) |
| sqlite | `DATE` | `datetime.date` | `str` | columns.Date |
| sqlite | `ARRAY_DATE` | `list[datetime.date]` | `str` | columns.Array(Date) |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | columns.Decimal |
| sqlite | `ARRAY` | `list[decimal.Decimal]` | `str` | columns.Array(Decimal) |
| sqlite | `ARRAY` | `list[float]` | `str` | columns.Array(DoublePrecision) |
| sqlite | `ARRAY` | `list[str]` | `str` | columns.Array(Email) |
| sqlite | `ARRAY` | `list[float]` | `str` | columns.Array(Float) |
| sqlite | `ARRAY` | `list[int]` | `str` | columns.Array(Integer) |
| sqlite | `SECONDS` | `datetime.timedelta` | `float` | columns.Interval |
| sqlite | `ARRAY` | `list[datetime.timedelta]` | `str` | columns.Array(Interval) |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | columns.Numeric |
| sqlite | `ARRAY` | `list[decimal.Decimal]` | `str` | columns.Array(Numeric) |
| sqlite | `ARRAY` | `list[float]` | `str` | columns.Array(Real) |
| sqlite | `ARRAY` | `list[str]` | `str` | columns.Array(Secret) |
| sqlite | `ARRAY` | `list[int]` | `str` | columns.Array(SmallInt) |
| sqlite | `ARRAY` | `list[str]` | `str` | columns.Array(Text) |
| sqlite | `TIME` | `datetime.time` | `str` | columns.Time |
| sqlite | `ARRAY_TIME` | `list[datetime.time]` | `str` | columns.Array(Time) |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | columns.Timestamp |
| sqlite | `ARRAY_TIMESTAMP` | `list[datetime.datetime]` | `str` | columns.Array(Timestamp) |
| sqlite | `TIMESTAMPTZ` | `datetime.datetime` | `str` | columns.Timestamptz |
| sqlite | `ARRAY_TIMESTAMPTZ` | `list[datetime.datetime]` | `str` | columns.Array(Timestamptz) |
| sqlite | `UUID` | `uuid.UUID` | `str` | columns.UUID |
| sqlite | `ARRAY` | `list[uuid.UUID]` | `str` | columns.Array(UUID) |
| sqlite | `ARRAY` | `list[str]` | `str` | columns.Array(Varchar) |
