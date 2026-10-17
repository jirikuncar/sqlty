# sqlalchemy 2.1.4 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| mysql | column | 28 | 2 | 2 | 5 |
| mysql | expression | 23 | 5 | 4 | 7 |
| postgres | column | 27 | 1 | 0 | 26 |
| postgres | expression | 27 | 5 | 0 | 7 |
| sqlite | column | 16 | 9 | 0 | 2 |
| sqlite | expression | 12 | 14 | 6 | 7 |
| **all** | | **133** | **36** | **12** | **54** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| postgres | `BIT(1)` | `sqlalchemy.dialects.postgresql.bitstring.BitString` | `str` | sqlalchemy.dialects.postgresql.BIT |
| postgres | `LOCALTIME` | `datetime.datetime` | `datetime.time` | func.localtime |
| postgres | `percent_rank()` | `decimal.Decimal` | `float` | func.percent_rank |
| postgres | `cume_dist()` | `decimal.Decimal` | `float` | func.cume_dist |
| postgres | `i %% n` | `int` | `decimal.Decimal` | int % numeric |
| postgres | `i || s` | `int` | `str` | int || text |
| mysql | `TIME` | `datetime.time` | `datetime.timedelta` | sqlalchemy.types.TIME |
| mysql | `CHAR(32)` | `uuid.UUID` | `str` | sqlalchemy.types.Uuid |
| mysql | `sum(i)` | `int` | `decimal.Decimal` | func.sum(int) |
| mysql | `percent_rank()` | `decimal.Decimal` | `float` | func.percent_rank |
| mysql | `cume_dist()` | `decimal.Decimal` | `float` | func.cume_dist |
| mysql | `i %% n` | `int` | `decimal.Decimal` | int % numeric |
| mysql | `concat(i, s)` | `int` | `str` | int || text |
| sqlite | `BOOLEAN` | `bool` | `int` | sqlalchemy.types.BOOLEAN |
| sqlite | `DATE` | `datetime.date` | `str` | sqlalchemy.types.DATE |
| sqlite | `DATETIME` | `datetime.datetime` | `str` | sqlalchemy.types.DATETIME |
| sqlite | `DECIMAL` | `decimal.Decimal` | `float` | sqlalchemy.types.DECIMAL |
| sqlite | `NUMERIC` | `decimal.Decimal` | `float` | sqlalchemy.types.NUMERIC |
| sqlite | `TIME` | `datetime.time` | `str` | sqlalchemy.types.TIME |
| sqlite | `TIMESTAMP` | `datetime.datetime` | `str` | sqlalchemy.types.TIMESTAMP |
| sqlite | `UUID` | `uuid.UUID` | `str` | sqlalchemy.types.UUID |
| sqlite | `CHAR(32)` | `uuid.UUID` | `str` | sqlalchemy.types.Uuid |
| sqlite | `sum(n)` | `decimal.Decimal` | `float` | func.sum(numeric) |
| sqlite | `max(d)` | `datetime.date` | `str` | func.max(date) |
| sqlite | `CURRENT_TIMESTAMP` | `datetime.datetime` | `str` | func.now |
| sqlite | `CURRENT_DATE` | `datetime.date` | `str` | func.current_date |
| sqlite | `CURRENT_TIMESTAMP` | `datetime.datetime` | `str` | func.current_timestamp |
| sqlite | `percent_rank()` | `decimal.Decimal` | `float` | func.percent_rank |
| sqlite | `cume_dist()` | `decimal.Decimal` | `float` | func.cume_dist |
| sqlite | `i + n` | `decimal.Decimal` | `float` | int + numeric |
| sqlite | `i / (i + 0.0)` | `decimal.Decimal` | `float` | int / int |
| sqlite | `n * n` | `decimal.Decimal` | `float` | numeric * numeric |
| sqlite | `i || s` | `int` | `str` | int || text |
| sqlite | `i = i` | `bool` | `int` | int = int |
| sqlite | `s LIKE 'a%'` | `bool` | `int` | text like |
| sqlite | `bo = 1 AND bo = 1` | `bool` | `int` | bool and |

## sqlty-gap

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| mysql | `ENUM()` | `str` | `None` | sqlalchemy.types.Enum |
| mysql | `SET()` | `str` | `None` | sqlalchemy.dialects.mysql.SET |
| mysql | `array_agg(i)` | `list` | `None` | func.array_agg |
| mysql | `d + iv` | `datetime.datetime` | `None` | date + interval |
| mysql | `ts - ts` | `datetime.timedelta` | `None` | datetime - datetime |
| mysql | `ts - iv` | `datetime.datetime` | `None` | datetime - interval |
| sqlite | `LOCALTIME` | `datetime.datetime` | `None` | func.localtime |
| sqlite | `CURRENT_USER` | `str` | `None` | func.current_user |
| sqlite | `array_agg(i)` | `list` | `None` | func.array_agg |
| sqlite | `d + iv` | `datetime.datetime` | `None` | date + interval |
| sqlite | `ts - ts` | `datetime.timedelta` | `None` | datetime - datetime |
| sqlite | `ts - iv` | `datetime.datetime` | `None` | datetime - interval |
