# MySQL nullability against the server's

336 columns, `scripts/compat/mysql_nullability.py`.

## Non-null in sqlty, nullable on MySQL

| Query | Column | sqlty | MySQL |
|-------|--------|-------|-------|
| `tests/mdtest/mysql/functions.md:71` | `g` | `int` | `bigint` |
| `tests/mdtest/mysql/functions.md:90` | `a` | `str` | `varchar(4)` |
| `tests/mdtest/mysql/functions.md:90` | `b` | `str` | `varchar(9)` |
| `tests/mdtest/mysql/functions.md:90` | `c` | `int` | `year` |
| `tests/mdtest/mysql/functions.md:90` | `d` | `int` | `int` |
| `tests/mdtest/mysql/functions.md:90` | `e` | `int` | `bigint` |
| `tests/mdtest/mysql/functions.md:126` | `a` | `str` | `varchar(101)` |
| `tests/mdtest/mysql/functions.md:126` | `b` | `str` | `varchar(5)` |
| `tests/mdtest/mysql/functions.md:126` | `c` | `str` | `varchar(2)` |
| `tests/mdtest/mysql/functions.md:126` | `d` | `str` | `varchar(50)` |
| `tests/mdtest/mysql/functions.md:126` | `e` | `str` | `varchar(32)` |
| `tests/mdtest/mysql/functions.md:126` | `h` | `str` | `varchar(36)` |
| `tests/mdtest/mysql/functions.md:148` | `a` | `bytes` | `varbinary(2)` |
| `tests/mdtest/mysql/functions.md:148` | `b` | `bytes` | `varbinary(16)` |
| `tests/mdtest/mysql/functions.md:148` | `c` | `bytes` | `varbinary(1)` |
| `tests/mdtest/mysql/functions.md:148` | `d` | `bytes` | `varbinary(16)` |
| `tests/mdtest/mysql/functions.md:148` | `e` | `bytes` | `varbinary(48)` |
| `tests/mdtest/mysql/functions.md:148` | `f` | `bytes` | `varbinary(3)` |
| `tests/mdtest/mysql/functions.md:169` | `e` | `int` | `int` |
| `tests/mdtest/mysql/functions.md:169` | `f` | `float` | `double` |
| `tests/mdtest/mysql/functions.md:169` | `g` | `float` | `double` |
| `tests/mdtest/mysql/functions.md:223` | `h` | `bytes` | `varbinary(16)` |
| `tests/mdtest/mysql/functions.md:223` | `i` | `str` | `varchar(65)` |
| `tests/mdtest/mysql/functions.md:257` | `c` | `decimal.Decimal` | `decimal(10,2)` |
| `tests/mdtest/mysql/functions.md:273` | `n` | `int` | `bigint` |
| `tests/mdtest/mysql/functions.md:273` | `has` | `int` | `int` |
| `tests/mdtest/mysql/functions.md:284` | `a` | `str` | `varchar(50)` |
| `tests/mdtest/mysql/functions.md:284` | `c` | `str` | `varchar(50)` |
| `tests/mdtest/mysql/functions.md:299` | `a` | `int` | `int` |
| `tests/mdtest/mysql/functions.md:299` | `b` | `int` | `int` |
| `tests/mdtest/mysql/functions.md:299` | `d` | `int` | `int` |
| `tests/mdtest/mysql/functions.md:299` | `e` | `datetime.date` | `date` |
| `tests/mdtest/mysql/functions.md:299` | `f` | `datetime.datetime` | `datetime` |
| `tests/mdtest/mysql/functions.md:316` | `a` | `str` | `varchar(127)` |
| `tests/mdtest/mysql/functions.md:316` | `e` | `bytes` | `blob` |
| `tests/mdtest/mysql/functions.md:316` | `f` | `float` | `double` |
| `tests/mdtest/mysql/functions.md:316` | `g` | `int` | `int` |
| `tests/mdtest/mysql/sqlglot_expressions.md:395` | `x` | `bytes` | `longblob` |
| `tests/mdtest/mysql/sqlglot_expressions.md:400` | `x` | `bytes` | `varbinary(34)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:405` | `x` | `bytes` | `varbinary(34)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:410` | `x` | `bytes` | `varbinary(29)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:415` | `x` | `bytes` | `varbinary(41)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:420` | `x` | `bytes` | `varbinary(28)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:425` | `x` | `bytes` | `varbinary(37)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:490` | `x` | `str` | `varchar(36)` |
| `tests/mdtest/mysql/sqlglot_expressions.md:495` | `x` | `str` | `longtext` |
| `tests/mdtest/mysql/type_spellings.md:52` | `CURRENT_USER` | `str` | `varchar(288)` |
| `tests/mdtest/mysql/type_spellings.md:52` | `u` | `str` | `varchar(288)` |
| `tests/mdtest/mysql/types.md:171` | `d` | `datetime.timedelta` | `time` |
| `tests/mdtest/mysql/types.md:171` | `c` | `datetime.timedelta` | `time` |
| `tests/mdtest/mysql/types.md:347` | `a` | `datetime.date` | `date` |
| `tests/mdtest/mysql/types.md:347` | `b` | `datetime.date` | `date` |
| `tests/mdtest/mysql/types.md:347` | `c` | `datetime.datetime` | `datetime` |
| `tests/mdtest/mysql/types.md:347` | `e` | `datetime.datetime` | `datetime` |
| `tests/mdtest/nullability/dialects.md:28` | `CAST(ifnull(nick, name) AS CHAR)` | `str` | `varchar(50)` |
| `tests/mdtest/nullability/dialects.md:35` | `CAST(concat(name, name) AS CHAR)` | `str` | `varchar(100)` |
| `tests/mdtest/nullability/dialects.md:42` | `CAST(if(age > 18, name, 'minor') AS CHAR)` | `str` | `varchar(50)` |
| `tests/mdtest/nullability/dialects.md:54` | `max(id)` | `int` | `int` |
| `tests/mdtest/nullability/dialects.md:139` | `x / 2` | `decimal.Decimal` | `decimal(14,4)` |
| `tests/mdtest/nullability/dialects.md:139` | `x % 2` | `int` | `int` |
| `tests/mdtest/nullability/narrowing.md:449` | `name` | `str` | `text` |
| `tests/mdtest/type_inference/columns.md:215` | `max(id)` | `int` | `int` |

## Nullable in sqlty, non-null on MySQL

| Query | Column | sqlty | MySQL |
|-------|--------|-------|-------|
| `tests/mdtest/mysql/functions.md:284` | `d` | `str \| None` | `varchar(50)` |
