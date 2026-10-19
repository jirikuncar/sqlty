# PostgreSQL nullability against seeded rows

877 columns of queries that return rows, `scripts/compat/pg_nullability.py`.

## NULL in a column sqlty types as non-null

None.

## Nullable in sqlty, never NULL in the rows

| Query | Column | sqlty |
|-------|--------|-------|
| `tests/mdtest/corpus/aggregates.md:50` | `names` | `str \| None` |
| `tests/mdtest/corpus/aggregates.md:65` | `total` | `float \| None` |
| `tests/mdtest/corpus/aggregates.md:97` | `col1` | `list[int] \| None` |
| `tests/mdtest/corpus/ctes.md:105` | `list` | `list[int] \| None` |
| `tests/mdtest/corpus/expressions.md:24` | `delta` | `decimal.Decimal \| None` |
| `tests/mdtest/corpus/expressions.md:24` | `delta_f` | `float \| None` |
| `tests/mdtest/corpus/expressions.md:24` | `delta_cast` | `float \| None` |
| `tests/mdtest/corpus/expressions.md:96` | `step` | `int \| None` |
| `tests/mdtest/corpus/json.md:72` | `teams` | `str \| None` |
| `tests/mdtest/corpus/outer_joins.md:157` | `bio` | `str \| None` |
| `tests/mdtest/corpus/outer_joins.md:165` | `bio` | `str \| None` |
| `tests/mdtest/corpus/outer_joins.md:208` | `first_name` | `str \| None` |
| `tests/mdtest/corpus/outer_joins.md:268` | `reference_id` | `uuid.UUID \| None` |
| `tests/mdtest/corpus/subqueries.md:24` | `g` | `int \| None` |
| `tests/mdtest/corpus/subqueries.md:24` | `h` | `int \| None` |
| `tests/mdtest/corpus/subqueries.md:24` | `m` | `str \| None` |
| `tests/mdtest/corpus/views.md:28` | `description` | `str \| None` |
| `tests/mdtest/corpus/views.md:73` | `tid` | `int \| None` |
| `tests/mdtest/nullability/inference.md:109` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:109` | `title` | `str \| None` |
| `tests/mdtest/nullability/inference.md:116` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:121` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:121` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:150` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:150` | `tag` | `str \| None` |
| `tests/mdtest/nullability/inference.md:160` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:160` | `tag` | `str \| None` |
| `tests/mdtest/nullability/inference.md:169` | `id` | `int \| None` |
| `tests/mdtest/nullability/inference.md:209` | `id` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:40` | `age` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:47` | `nick` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:47` | `prefs` | `typing.Any` |
| `tests/mdtest/nullability/narrowing.md:201` | `id` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:225` | `id` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:243` | `id` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:264` | `id` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:264` | `id` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:293` | `n` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:298` | `n` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:304` | `n` | `int \| None` |
| `tests/mdtest/nullability/narrowing.md:332` | `motto` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:353` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:358` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:363` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:368` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:373` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:409` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:409` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:409` | `name` | `str \| None` |
| `tests/mdtest/nullability/narrowing.md:469` | `?column?` | `bool \| None` |
| `tests/mdtest/nullability/postgres.md:66` | `nullif` | `int \| None` |
| `tests/mdtest/nullability/postgres.md:66` | `nullif` | `str \| None` |
| `tests/mdtest/nullability/postgres.md:104` | `case` | `str \| None` |
| `tests/mdtest/nullability/postgres.md:117` | `max` | `int \| None` |
| `tests/mdtest/nullability/postgres.md:117` | `sum` | `int \| None` |
| `tests/mdtest/nullability/postgres.md:117` | `avg` | `decimal.Decimal \| None` |
| `tests/mdtest/nullability/postgres.md:145` | `array_agg` | `list[int] \| None` |
| `tests/mdtest/nullability/postgres.md:169` | `max` | `decimal.Decimal \| None` |
| `tests/mdtest/nullability/postgres.md:179` | `total` | `decimal.Decimal \| None` |
| `tests/mdtest/nullability/sets_and_dml.md:214` | `task` | `str \| None` |
| `tests/mdtest/nullability/sets_and_dml.md:219` | `task` | `str \| None` |
| `tests/mdtest/nullability/sets_and_dml.md:232` | `task` | `str \| None` |
| `tests/mdtest/nullability/sets_and_dml.md:237` | `task` | `str \| None` |
| `tests/mdtest/nullability/soundness.md:96` | `max` | `int \| None` |
| `tests/mdtest/nullability/soundness.md:117` | `a` | `int \| None` |
| `tests/mdtest/nullability/soundness.md:169` | `last_value` | `int \| None` |
| `tests/mdtest/nullability/soundness.md:225` | `?column?` | `bool \| None` |
| `tests/mdtest/nullability/soundness.md:353` | `Email` | `str \| None` |
| `tests/mdtest/postgres/aggregates.md:89` | `max` | `str \| None` |
| `tests/mdtest/postgres/aggregates.md:89` | `min` | `str \| None` |
| `tests/mdtest/postgres/aggregates.md:89` | `max` | `str \| None` |
| `tests/mdtest/postgres/builtins.md:79` | `a` | `int \| None` |
| `tests/mdtest/postgres/builtins.md:79` | `o` | `int \| None` |
| `tests/mdtest/postgres/builtins.md:79` | `c` | `float \| None` |
| `tests/mdtest/postgres/builtins.md:79` | `cp` | `float \| None` |
| `tests/mdtest/postgres/builtins.md:79` | `rs` | `float \| None` |
| `tests/mdtest/postgres/builtins.md:89` | `p` | `float \| None` |
| `tests/mdtest/postgres/builtins.md:89` | `pi` | `datetime.timedelta \| None` |
| `tests/mdtest/postgres/builtins.md:89` | `pd` | `str \| None` |
| `tests/mdtest/postgres/builtins.md:89` | `mo` | `datetime.date \| None` |
| `tests/mdtest/postgres/builtins.md:89` | `ps` | `list[float] \| None` |
| `tests/mdtest/postgres/builtins.md:199` | `j` | `str \| None` |
| `tests/mdtest/postgres/builtins.md:226` | `g` | `bool \| None` |
| `tests/mdtest/postgres/builtins.md:226` | `h` | `typing.Any` |
| `tests/mdtest/postgres/builtins.md:226` | `m` | `None` |
| `tests/mdtest/postgres/builtins.md:241` | `r` | `None` |
| `tests/mdtest/postgres/builtins.md:251` | `g` | `None` |
| `tests/mdtest/postgres/builtins.md:251` | `h` | `None` |
| `tests/mdtest/postgres/builtins.md:251` | `k` | `None` |
| `tests/mdtest/postgres/builtins.md:271` | `headline` | `typing.Any` |
| `tests/mdtest/postgres/builtins.md:298` | `headline` | `typing.Any` |
| `tests/mdtest/postgres/builtins.md:298` | `bytes` | `bytes \| None` |
| `tests/mdtest/postgres/builtins.md:298` | `grid` | `list[list[int]] \| None` |
| `tests/mdtest/postgres/builtins.md:298` | `element` | `int \| None` |
| `tests/mdtest/postgres/builtins.md:322` | `lsn` | `str \| None` |
| `tests/mdtest/postgres/builtins.md:322` | `merged` | `None` |
| `tests/mdtest/postgres/builtins.md:322` | `multi` | `None` |
| `tests/mdtest/postgres/builtins.md:322` | `low` | `decimal.Decimal \| None` |
| `tests/mdtest/postgres/builtins.md:351` | `crossing` | `str \| None` |
| `tests/mdtest/postgres/check_literals.md:261` | `status` | `None` |
| `tests/mdtest/postgres/column_names.md:23` | `max` | `int \| None` |
| `tests/mdtest/postgres/column_names.md:72` | `extract` | `decimal.Decimal \| None` |
| `tests/mdtest/postgres/derived_tables.md:55` | `created_at` | `datetime.datetime \| None` |
| `tests/mdtest/postgres/derived_tables.md:100` | `created_at` | `datetime.datetime \| None` |
| `tests/mdtest/postgres/derived_tables.md:107` | `status` | `str \| None` |
| `tests/mdtest/postgres/domains_and_enums.md:98` | `discount` | `int \| None` |
| `tests/mdtest/postgres/json_shapes.md:171` | `items` | `typing.Any` |
| `tests/mdtest/postgres/json_shapes.md:270` | `a` | `str \| None` |
| `tests/mdtest/postgres/json_typing.md:125` | `scalar` | `typing.Any` |
| `tests/mdtest/postgres/locking.md:45` | `status` | `str \| None` |
| `tests/mdtest/postgres/python_types.md:38` | `avg` | `decimal.Decimal \| None` |
| `tests/mdtest/postgres/python_types.md:38` | `avg` | `float \| None` |
| `tests/mdtest/postgres/python_types.md:45` | `tags` | `typing.Any` |
| `tests/mdtest/postgres/returning.md:222` | `label` | `str \| None` |
| `tests/mdtest/postgres/schema/alter_table.md:86` | `age` | `int \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:63` | `table_schema` | `str \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:63` | `table_name` | `str \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:63` | `table_type` | `str \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:68` | `column_name` | `str \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:68` | `ordinal_position` | `int \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:68` | `is_nullable` | `str \| None` |
| `tests/mdtest/postgres/schema/builtin_catalog.md:68` | `data_type` | `str \| None` |
| `tests/mdtest/postgres/schema/catalog_refresh.md:28` | `x` | `int \| None` |
| `tests/mdtest/postgres/schema/catalog_refresh.md:33` | `x` | `None` |
| `tests/mdtest/postgres/schema/create_if_not_exists.md:64` | `f` | `str \| None` |
| `tests/mdtest/postgres/schema/functions.md:46` | `loose` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:46` | `called` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:65` | `f` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:65` | `f` | `str \| None` |
| `tests/mdtest/postgres/schema/functions.md:75` | `g` | `None` |
| `tests/mdtest/postgres/schema/functions.md:89` | `answer` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:106` | `f` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:127` | `current_status` | `typing.Literal['new', 'done'] \| None` |
| `tests/mdtest/postgres/schema/functions.md:127` | `share` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:152` | `id` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:152` | `name` | `str \| None` |
| `tests/mdtest/postgres/schema/functions.md:152` | `seen` | `datetime.datetime \| None` |
| `tests/mdtest/postgres/schema/functions.md:157` | `id` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:157` | `seen` | `datetime.datetime \| None` |
| `tests/mdtest/postgres/schema/functions.md:172` | `id` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:177` | `id` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:182` | `lo` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:182` | `hi` | `int \| None` |
| `tests/mdtest/postgres/schema/functions.md:199` | `f` | `None` |
| `tests/mdtest/postgres/set_returning_functions.md:69` | `unnest` | `str \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:74` | `n` | `int \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:74` | `s` | `str \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:94` | `value` | `typing.Any` |
| `tests/mdtest/postgres/set_returning_functions.md:99` | `value` | `str \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:111` | `v` | `str \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:135` | `a` | `decimal.Decimal \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:183` | `x` | `int \| None` |
| `tests/mdtest/postgres/set_returning_functions.md:196` | `unnest` | `str \| None` |
