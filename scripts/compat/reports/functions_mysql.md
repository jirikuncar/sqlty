# MySQL 8.4.13 functions

468 queries the engine accepts, `scripts/compat/engine_functions.py mysql`. 489 columns agree with the engine.

## Typed differently

| Query | sqlty | Engine |
|---|---|---|
| `SELECT BINARY 'a' = 'A'` | `bytes` (BINARY) | `int` (`int`) |
| `SELECT BINARY 'a' = 'a '` | `bytes` (BINARY) | `int` (`int`) |
| `SELECT UNIX_TIMESTAMP('2015-11-13 10:20:19.012')` | `int` (BIGINT) | `decimal(14,3)` (`decimal.Decimal`) |

## Untyped (1 functions)

`STR_TO_DATE`

## Untyped for some arguments (10 functions)

`COALESCE`, `CURDATE`, `CURTIME`, `FROM_UNIXTIME`, `NOW`, `SEC_TO_TIME`, `UTC_DATE`, `UTC_TIME`, `UTC_TIMESTAMP`, `|`

## Not parsed (5 queries)

| Query |
|---|
| `SELECT DATE_ADD('1900-01-01 00:00:00', INTERVAL '-1 10' DAY_HOUR)` |
| `SELECT DATE_ADD('2100-12-31 23:59:59', INTERVAL '1:1' MINUTE_SECOND)` |
| `SELECT DATE_SUB('2025-01-01 00:00:00', INTERVAL '1 1:1:1' DAY_SECOND)` |
| `SELECT EXTRACT(DAY_MINUTE FROM '2019-07-02 01:02:03')` |
| `SELECT EXTRACT(YEAR_MONTH FROM '2019-07-02 01:02:03')` |
