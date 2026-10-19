# sqlglot 30.23.0 vs sqlty

| Dialect | Kind | Match | Mismatch | sqlty gap | Not comparable |
|---------|------|-------|----------|-----------|----------------|
| generic | expression | 41 | 4 | 70 | 2 |
| mysql | expression | 97 | 0 | 0 | 78 |
| postgres | expression | 46 | 0 | 0 | 6 |
| **all** | | **184** | **4** | **70** | **86** |

## mismatch

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| generic | `ABS(1.5)` | `float` | `decimal.Decimal` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `5.3` | `float` | `decimal.Decimal` | annotate_types.sql: sqlglot DOUBLE |
| generic | `ALL(SELECT 1)` | `bool` | `int` | annotate_types.sql: sqlglot bool |
| generic | `1 + (SELECT 2.5 AS c)` | `float` | `decimal.Decimal` | annotate_types.sql: sqlglot DOUBLE |

## sqlty-gap

| Dialect | SQL | Expected | sqlty | Provenance |
|---------|-----|----------|-------|------------|
| generic | `GREATEST(1, 2, 3)` | `int` | `None` | annotate_functions.sql: sqlglot INT |
| generic | `GREATEST(1, 2.5, 3)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `LEAST(1, 2, 3)` | `int` | `None` | annotate_functions.sql: sqlglot INT |
| generic | `LEAST(1, 2.5, 3)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `LOCALTIME()` | `datetime.time` | `None` | annotate_functions.sql: sqlglot TIME |
| generic | `TIME_ADD(CAST('09:05:03' AS TIME), INTERVAL 2 HOUR)` | `datetime.time` | `None` | annotate_functions.sql: sqlglot TIME |
| generic | `TIME_SUB(CAST('09:05:03' AS TIME), INTERVAL 2 HOUR)` | `datetime.time` | `None` | annotate_functions.sql: sqlglot TIME |
| generic | `SORT_ARRAY(ARRAY(tbl.str_col))` | `list[str]` | `None` | annotate_functions.sql: sqlglot ARRAY<STRING> |
| generic | `SORT_ARRAY(ARRAY(tbl.double_col))` | `list[float]` | `None` | annotate_functions.sql: sqlglot ARRAY<DOUBLE> |
| generic | `SORT_ARRAY(ARRAY(tbl.bigint_col))` | `list[int]` | `None` | annotate_functions.sql: sqlglot ARRAY<BIGINT> |
| generic | `ARRAY_REVERSE(['a', 'b'])` | `list[str]` | `None` | annotate_functions.sql: sqlglot ARRAY<VARCHAR> |
| generic | `ARRAY_REVERSE([1, 1.5])` | `list[float]` | `None` | annotate_functions.sql: sqlglot ARRAY<DOUBLE> |
| generic | `ARRAY_SLICE([1, 1.5], 1, 2)` | `list[float]` | `None` | annotate_functions.sql: sqlglot ARRAY<DOUBLE> |
| generic | `FROM_BASE32(tbl.str_col)` | `bytes` | `None` | annotate_functions.sql: sqlglot BINARY |
| generic | `FROM_BASE64(tbl.str_col)` | `bytes` | `None` | annotate_functions.sql: sqlglot BINARY |
| generic | `ANY_VALUE(tbl.bool_col)` | `bool` | `None` | annotate_functions.sql: sqlglot BOOLEAN |
| generic | `ANY_VALUE(tbl.bigint_col)` | `int` | `None` | annotate_functions.sql: sqlglot BIGINT |
| generic | `ANY_VALUE(tbl.date_col)` | `datetime.date` | `None` | annotate_functions.sql: sqlglot DATE |
| generic | `ANY_VALUE(tbl.str_col)` | `str` | `None` | annotate_functions.sql: sqlglot STRING |
| generic | `ANY_VALUE(tbl.array_col)` | `list[str]` | `None` | annotate_functions.sql: sqlglot ARRAY<STRING> |
| generic | `CURRENT_SCHEMA()` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `MONTHNAME(tbl.date_col)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `CHR(65)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `COUNTIF(tbl.bigint_col > 1)` | `int` | `None` | annotate_functions.sql: sqlglot BIGINT |
| generic | `TO_BASE32(tbl.bytes_col)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `TO_BASE64(tbl.bytes_col)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `UNIX_SECONDS(tbl.timestamp_col)` | `int` | `None` | annotate_functions.sql: sqlglot BIGINT |
| generic | `STARTS_WITH(tbl.str_col, prefix)` | `bool` | `None` | annotate_functions.sql: sqlglot BOOLEAN |
| generic | `ENDS_WITH(tbl.str_col, suffix)` | `bool` | `None` | annotate_functions.sql: sqlglot BOOLEAN |
| generic | `ASCII('A')` | `int` | `None` | annotate_functions.sql: sqlglot INT |
| generic | `UNICODE('bcd')` | `int` | `None` | annotate_functions.sql: sqlglot INT |
| generic | `LAST_DAY(tbl.timestamp_col)` | `datetime.date` | `None` | annotate_functions.sql: sqlglot DATE |
| generic | `JUSTIFY_DAYS(INTERVAL '1' DAY)` | `datetime.timedelta` | `None` | annotate_functions.sql: sqlglot INTERVAL |
| generic | `JUSTIFY_HOURS(INTERVAL '1' HOUR)` | `datetime.timedelta` | `None` | annotate_functions.sql: sqlglot INTERVAL |
| generic | `JUSTIFY_INTERVAL(INTERVAL '1' HOUR)` | `datetime.timedelta` | `None` | annotate_functions.sql: sqlglot INTERVAL |
| generic | `UNIX_MICROS(CAST('2008-12-25 15:30:00+00' AS TIMESTAMP))` | `int` | `None` | annotate_functions.sql: sqlglot BIGINT |
| generic | `UNIX_MILLIS(CAST('2008-12-25 15:30:00+00' AS TIMESTAMP))` | `int` | `None` | annotate_functions.sql: sqlglot BIGINT |
| generic | `KURTOSIS(tbl.double_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `KURTOSIS(tbl.int_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `DAYNAME(tbl.date_col)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `CBRT(tbl.int_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `CBRT(tbl.double_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `ISINF(tbl.float_col)` | `bool` | `None` | annotate_functions.sql: sqlglot BOOLEAN |
| generic | `ISNAN(tbl.float_col)` | `bool` | `None` | annotate_functions.sql: sqlglot BOOLEAN |
| generic | `CURRENT_CATALOG()` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `CURRENT_USER()` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `SESSION_USER()` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `RAND()` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `ARRAY_CONTAINS(tbl.array_col, '1')` | `bool` | `None` | annotate_functions.sql: sqlglot BOOLEAN |
| generic | `REPEAT(tbl.str_col, tbl.int_col)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `TYPEOF(foo)` | `str` | `None` | annotate_functions.sql: sqlglot VARCHAR |
| generic | `COVAR_POP(tbl.double_col, tbl.double_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `COVAR_POP(tbl.int_col, tbl.int_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `COVAR_SAMP(tbl.double_col, tbl.double_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `COVAR_SAMP(tbl.int_col, tbl.int_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY tbl.double_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY tbl.double_col)` | `float` | `None` | annotate_functions.sql: sqlglot DOUBLE |
| generic | `PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY tbl.int_col)` | `int` | `None` | annotate_functions.sql: sqlglot INT |
| generic | `PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY tbl.bigint_col)` | `int` | `None` | annotate_functions.sql: sqlglot BIGINT |
| generic | `~5` | `int` | `None` | annotate_types.sql: sqlglot INT |
| generic | `ANY(SELECT 1)` | `bool` | `None` | annotate_types.sql: sqlglot bool |
| generic | `IF(true, 1, null)` | `int` | `None` | annotate_types.sql: sqlglot INT |
| generic | `IF(true, null, 1)` | `int` | `None` | annotate_types.sql: sqlglot INT |
| generic | `STRUCT(1 AS col)` | `dict[str, int]` | `None` | annotate_types.sql: sqlglot STRUCT<col INT> |
| generic | `STRUCT(1 AS col, 2.5 AS row)` | `dict[str, int | float]` | `None` | annotate_types.sql: sqlglot STRUCT<col INT, row DOUBLE> |
| generic | `STRUCT(1)` | `dict[str, int]` | `None` | annotate_types.sql: sqlglot STRUCT<INT> |
| generic | `STRUCT(1 AS col, 2.5 AS row, struct(3.5 AS inner_col, 4 AS inner_row) AS nested_struct)` | `dict[str, int | float | dict[str, float | int]]` | `None` | annotate_types.sql: sqlglot STRUCT<col INT, row DOUBLE, nested_struct STRUCT<inner_col DOUBLE, inner_row INT>> |
| generic | `STRUCT(1 AS col, 2.5, ARRAY[1, 2, 3] AS nested_array, 'foo')` | `dict[str, int | float | list[int] | str]` | `None` | annotate_types.sql: sqlglot STRUCT<col INT, DOUBLE, nested_array ARRAY<INT>, VARCHAR> |
| generic | `STRUCT(1, 2.5, 'bar')` | `dict[str, int | float | str]` | `None` | annotate_types.sql: sqlglot STRUCT<INT, DOUBLE, VARCHAR> |
| generic | `STRUCT(1 AS "CaseSensitive")` | `dict[str, int]` | `None` | annotate_types.sql: sqlglot STRUCT<"CaseSensitive" INT> |
