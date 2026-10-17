# django 6.1.2 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 23 | 3 | 0 | 0 |
| mysql | expression | 4 | 3 | 1 | 0 |
| postgres | column | 24 | 2 | 0 | 0 |
| postgres | expression | 7 | 1 | 0 | 0 |
| sqlite | column | 19 | 7 | 0 | 0 |
| sqlite | expression | 4 | 3 | 1 | 0 |
| **all** | | **81** | **19** | **2** | **0** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| postgres | `inet` | `str` | `ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface` | IPAddressField |
| postgres | `inet` | `str` | `ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface` | GenericIPAddressField |
| postgres | `avg(i)` | `float` | `decimal.Decimal` | output_field FloatField |
| mysql | `bigint` | `datetime.timedelta` | `int` | DurationField |
| mysql | `time(6)` | `datetime.time` | `datetime.timedelta` | TimeField |
| mysql | `char(32)` | `uuid.UUID` | `str` | UUIDField |
| mysql | `sum(i)` | `int` | `decimal.Decimal` | output_field IntegerField |
| mysql | `avg(i)` | `float` | `decimal.Decimal` | output_field FloatField |
| mysql | `i / i` | `int` | `decimal.Decimal` | output_field IntegerField |
| sqlite | `bool` | `bool` | `int` | BooleanField |
| sqlite | `date` | `datetime.date` | `str` | DateField |
| sqlite | `datetime` | `datetime.datetime` | `str` | DateTimeField |
| sqlite | `decimal` | `decimal.Decimal` | `float` | DecimalField |
| sqlite | `bigint` | `datetime.timedelta` | `int` | DurationField |
| sqlite | `time` | `datetime.time` | `str` | TimeField |
| sqlite | `char(32)` | `uuid.UUID` | `str` | UUIDField |
| sqlite | `avg(d)` | `decimal.Decimal` | `float` | output_field DecimalField |
| sqlite | `max(d)` | `decimal.Decimal` | `float` | output_field DecimalField |
| sqlite | `i * d` | `decimal.Decimal` | `float` | output_field DecimalField |

## sqlty-gap

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| mysql | `ts - ts` | `datetime.timedelta` | `None` | output_field DurationField |
| sqlite | `ts - ts` | `datetime.timedelta` | `None` | output_field DurationField |
