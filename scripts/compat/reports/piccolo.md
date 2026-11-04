# piccolo 1.36.0 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| postgres | column | 48 | 0 | 0 | 0 |
| sqlite | column | 17 | 13 | 18 | 0 |
| **all** | | **65** | **13** | **18** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `BOOLEAN` | `bool` | `int` | columns.Boolean |
| sqlite | `DATE` | `datetime.date` | `str` | columns.Date |
| sqlite | `ARRAY_DATE` | `list[datetime.date]` | `float` | columns.Array(Date) |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | columns.Decimal |
| sqlite | `SECONDS` | `datetime.timedelta` | `float` | columns.Interval |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | columns.Numeric |
| sqlite | `TIME` | `datetime.time` | `str` | columns.Time |
| sqlite | `ARRAY_TIME` | `list[datetime.time]` | `float` | columns.Array(Time) |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | columns.Timestamp |
| sqlite | `ARRAY_TIMESTAMP` | `list[datetime.datetime]` | `float` | columns.Array(Timestamp) |
| sqlite | `TIMESTAMPTZ` | `datetime.datetime` | `str` | columns.Timestamptz |
| sqlite | `ARRAY_TIMESTAMPTZ` | `list[datetime.datetime]` | `float` | columns.Array(Timestamptz) |
| sqlite | `UUID` | `uuid.UUID` | `str` | columns.UUID |

## sqlty-gap

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| sqlite | `ARRAY` | `list[int]` | `None` | columns.Array(BigInt) |
| sqlite | `ARRAY` | `list[bytes]` | `None` | columns.Array(Blob) |
| sqlite | `ARRAY` | `list[bool]` | `None` | columns.Array(Boolean) |
| sqlite | `ARRAY` | `list[bytes]` | `None` | columns.Array(Bytea) |
| sqlite | `ARRAY` | `list[str]` | `None` | columns.Array(Char) |
| sqlite | `ARRAY` | `list[decimal.Decimal]` | `None` | columns.Array(Decimal) |
| sqlite | `ARRAY` | `list[float]` | `None` | columns.Array(DoublePrecision) |
| sqlite | `ARRAY` | `list[str]` | `None` | columns.Array(Email) |
| sqlite | `ARRAY` | `list[float]` | `None` | columns.Array(Float) |
| sqlite | `ARRAY` | `list[int]` | `None` | columns.Array(Integer) |
| sqlite | `ARRAY` | `list[datetime.timedelta]` | `None` | columns.Array(Interval) |
| sqlite | `ARRAY` | `list[decimal.Decimal]` | `None` | columns.Array(Numeric) |
| sqlite | `ARRAY` | `list[float]` | `None` | columns.Array(Real) |
| sqlite | `ARRAY` | `list[str]` | `None` | columns.Array(Secret) |
| sqlite | `ARRAY` | `list[int]` | `None` | columns.Array(SmallInt) |
| sqlite | `ARRAY` | `list[str]` | `None` | columns.Array(Text) |
| sqlite | `ARRAY` | `list[uuid.UUID]` | `None` | columns.Array(UUID) |
| sqlite | `ARRAY` | `list[str]` | `None` | columns.Array(Varchar) |
