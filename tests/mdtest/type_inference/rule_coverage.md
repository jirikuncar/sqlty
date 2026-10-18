# Every inference rule

Each rule of `rust/crates/sqlty-sql/rules/` types a column below, over one table per
dialect with a column of each type. `test_every_inference_rule_is_asserted` fails when
a rule has no `-- rules:` assertion, so removing or shadowing a rule fails a test.

## PostgreSQL

`schema.sql`:

```sql
CREATE TABLE t (
    i2 SMALLINT NOT NULL, i4 INT NOT NULL, i8 BIGINT NOT NULL, n NUMERIC NOT NULL,
    r REAL NOT NULL, d DOUBLE PRECISION NOT NULL, tx TEXT NOT NULL, vc VARCHAR(10) NOT NULL,
    b BOOLEAN NOT NULL, j JSON NOT NULL, jb JSONB NOT NULL, dt DATE NOT NULL,
    ts TIMESTAMP NOT NULL, tz TIMESTAMPTZ NOT NULL, tm TIME NOT NULL, ttz TIMETZ NOT NULL,
    iv INTERVAL NOT NULL, bt BYTEA NOT NULL, ia INT[] NOT NULL, ip INET NOT NULL,
    cn CIDR NOT NULL, rg INT4RANGE NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

### Aggregates

```sql
SELECT AVG(d) AS avg_real, AVG(iv) AS avg_interval, AVG(i4) AS avg, AVG(NULL) AS avg_untyped,
    MIN(i4) AS min, SUM(iv) AS sum, SUM(i8) AS sum_exact, SUM(r) AS sum_real,
    SUM(d) AS sum_double, MIN(vc) AS min_varchar
FROM t
-- rules: avg_real: common.aggregate.avg-real / common.aggregate.avg-real
-- rules: avg_interval: common.aggregate.avg-interval / common.aggregate.avg-interval
-- rules: avg: common.aggregate.avg / common.aggregate.avg
-- rules: avg_untyped: common.aggregate.avg-untyped / common.aggregate.avg-untyped
-- rules: min: common.aggregate.min-max / common.aggregate.min-max
-- rules: sum: common.aggregate.sum / common.aggregate.sum
-- rules: sum_exact: postgres.aggregate.sum-exact / postgres.aggregate.sum-exact
-- rules: sum_real: postgres.aggregate.sum-real / postgres.aggregate.sum-real
-- rules: sum_double: postgres.aggregate.sum-double / postgres.aggregate.sum-double
-- rules: min_varchar: postgres.aggregate.min-max-varchar / postgres.aggregate.min-max-varchar
```

### Arithmetic

```sql
SELECT n ^ 2 AS power_numeric, i4 ^ n AS power_integer_numeric, d ^ 2 AS power,
    r + r AS reals, r + i4 AS real_and_other, i4 + r AS other_and_real,
    i4 / i8 AS division, tx / tx AS division_other
FROM t
-- rules: power_numeric: common.arithmetic.power-numeric / common.arithmetic.power-numeric
-- rules: power_integer_numeric: common.arithmetic.power-integer-numeric / common.arithmetic.power-integer-numeric
-- rules: power: common.arithmetic.power / common.arithmetic.power
-- rules: reals: postgres.arithmetic.reals / postgres.arithmetic.reals
-- rules: real_and_other: postgres.arithmetic.real-and-other / postgres.arithmetic.real-and-other
-- rules: other_and_real: postgres.arithmetic.other-and-real / postgres.arithmetic.other-and-real
-- rules: division: postgres.arithmetic.division / postgres.arithmetic.division
-- rules: division_other: postgres.arithmetic.division-other / postgres.arithmetic.division-other
```

### Dates and times

```sql
SELECT dt + iv AS date_interval, ts + iv AS timestamp_interval, iv + dt AS interval_date,
    iv + ts AS interval_timestamp, i4 + dt AS days_date, dt + tm AS date_time,
    tm + dt AS time_date, dt + ttz AS date_timetz, ttz + dt AS timetz_date,
    dt - dt AS date_difference, tm - tm AS time_difference, ts - ts AS timestamp_difference,
    tz - tz AS timestamptz_difference, ts - dt AS timestamp_date_difference,
    dt - ts AS date_timestamp_difference, iv + iv AS intervals, iv * 2 AS interval_scaled,
    2 * iv AS scaled_interval
FROM t
-- rules: date_interval: postgres.datetime.date-interval / postgres.datetime.date-interval
-- rules: timestamp_interval: postgres.datetime.timestamp-interval / postgres.datetime.timestamp-interval
-- rules: interval_date: postgres.datetime.interval-date / postgres.datetime.interval-date
-- rules: interval_timestamp: postgres.datetime.interval-timestamp / postgres.datetime.interval-timestamp
-- rules: days_date: postgres.datetime.days-date / postgres.datetime.days-date
-- rules: date_time: postgres.datetime.date-time / postgres.datetime.date-time
-- rules: time_date: postgres.datetime.time-date / postgres.datetime.time-date
-- rules: date_timetz: postgres.datetime.date-timetz / postgres.datetime.date-timetz
-- rules: timetz_date: postgres.datetime.timetz-date / postgres.datetime.timetz-date
-- rules: date_difference: postgres.datetime.date-difference / postgres.datetime.date-difference
-- rules: time_difference: postgres.datetime.time-difference / postgres.datetime.time-difference
-- rules: timestamp_difference: postgres.datetime.timestamp-difference / postgres.datetime.timestamp-difference
-- rules: timestamptz_difference: postgres.datetime.timestamptz-difference / postgres.datetime.timestamptz-difference
-- rules: timestamp_date_difference: postgres.datetime.timestamp-date-difference / postgres.datetime.timestamp-date-difference
-- rules: date_timestamp_difference: postgres.datetime.date-timestamp-difference / postgres.datetime.date-timestamp-difference
-- rules: intervals: postgres.datetime.intervals / postgres.datetime.intervals
-- rules: interval_scaled: postgres.datetime.interval-scaled / postgres.datetime.interval-scaled
-- rules: scaled_interval: postgres.datetime.scaled-interval / postgres.datetime.scaled-interval
```

### Network addresses, bits and ranges

```sql
SELECT ip - ip AS difference, ip + 1 AS "offset", 1 + ip AS offset_left,
    ip << cn AS address_shift, '10.0.0.0/8' >> ip AS address_shift_right,
    ip & ip AS address_mask, '255.0.0.0' & ip AS address_mask_right, i4 & i8 AS integers,
    i4 << 2 AS shift, ip <<= cn AS contained_or_equal, rg -|- rg AS adjacency,
    rg + rg AS set_operations
FROM t
-- rules: difference: postgres.network.difference / postgres.network.difference
-- rules: offset: postgres.network.offset / postgres.network.offset
-- rules: offset_left: postgres.network.offset-left / postgres.network.offset-left
-- rules: address_shift: postgres.bitwise.address-shift / postgres.bitwise.address-shift
-- rules: address_shift_right: postgres.bitwise.address-shift-right-operand / postgres.bitwise.address-shift-right-operand
-- rules: address_mask: postgres.bitwise.address-mask / postgres.bitwise.address-mask
-- rules: address_mask_right: postgres.bitwise.address-mask-right-operand / postgres.bitwise.address-mask-right-operand
-- rules: integers: postgres.bitwise.integers / postgres.bitwise.integers
-- rules: shift: postgres.bitwise.shift / postgres.bitwise.shift
-- rules: contained_or_equal: postgres.network.contained-or-equal / postgres.network.contained-or-equal
-- rules: adjacency: postgres.range.adjacency / postgres.range.adjacency
-- rules: set_operations: postgres.range.set-operations / postgres.range.set-operations
```

### Comparison, logic and patterns

```sql
SELECT i4 = i8 AS comparison, b AND b AS and_or, tx ~ 'a' AS pattern, ia @> ia AS containment
FROM t
-- rules: comparison: common.comparison / common.comparison
-- rules: and_or: common.logical.and-or / common.logical.and-or
-- rules: pattern: common.pattern / common.pattern
-- rules: containment: common.containment / common.containment
```

### Concatenation

```sql
SELECT jb || jb AS jsonb, '{}' || jb AS jsonb_right, ia || ia AS arrays, ia || 1 AS array,
    1 || ia AS array_right, bt || bt AS bytea, tx || i4 AS text, i4 || tx AS text_right
FROM t
-- rules: jsonb: postgres.concat.jsonb / postgres.concat.jsonb
-- rules: jsonb_right: postgres.concat.jsonb-right-operand / postgres.concat.jsonb-right-operand
-- rules: arrays: postgres.concat.arrays / postgres.concat.arrays
-- rules: array: postgres.concat.array / postgres.concat.array
-- rules: array_right: postgres.concat.array-right / postgres.concat.array-right
-- rules: bytea: postgres.concat.bytea / postgres.concat.bytea
-- rules: text: common.concat.text / common.concat.text
-- rules: text_right: common.concat.text-right / common.concat.text-right
```

### JSON

```sql
SELECT j -> 'a' AS arrow_json, tx -> 'a' AS arrow, j #> '{a}' AS hash_arrow_json,
    jb #> '{a}' AS hash_arrow_jsonb, tx #> '{a}' AS hash_arrow, j ->> 'a' AS long_arrow_json,
    j #>> '{a}' AS hash_long_arrow_json, tx ->> 'a' AS long_arrow,
    tx #>> '{a}' AS hash_long_arrow, j #- '{a}' AS hash_minus_json, tx #- '{a}' AS hash_minus,
    jb - 'a' AS minus, jb ? 'a' AS key_exists, jb ?| ARRAY['a'] AS keys_exist,
    jb @> jb AS contains
FROM t
-- rules: arrow_json: postgres.json.arrow-json / postgres.json.arrow-json
-- rules: arrow: postgres.json.arrow / postgres.json.arrow
-- rules: hash_arrow_json: postgres.json.hash-arrow-json / postgres.json.hash-arrow-json
-- rules: hash_arrow_jsonb: postgres.json.hash-arrow-jsonb / postgres.json.hash-arrow-jsonb
-- rules: hash_arrow: postgres.json.hash-arrow / postgres.json.hash-arrow
-- rules: long_arrow_json: common.json.long-arrow-json / common.json.long-arrow-json
-- rules: hash_long_arrow_json: common.json.hash-long-arrow-json / common.json.hash-long-arrow-json
-- rules: long_arrow: common.json.long-arrow / common.json.long-arrow
-- rules: hash_long_arrow: common.json.hash-long-arrow / common.json.hash-long-arrow
-- rules: hash_minus_json: postgres.json.hash-minus-json / postgres.json.hash-minus-json
-- rules: hash_minus: postgres.json.hash-minus / postgres.json.hash-minus
-- rules: minus: postgres.json.minus / postgres.json.minus
-- rules: key_exists: postgres.json.key-exists / postgres.json.key-exists
-- rules: keys_exist: postgres.json.keys-exist / postgres.json.keys-exist
-- rules: contains: postgres.json.contains / postgres.json.contains
```

## MySQL

`schema.sql`:

```sql
CREATE TABLE t (
    i INT NOT NULL, ui INT UNSIGNED NOT NULL, n DECIMAL(10, 2) NOT NULL,
    d DOUBLE NOT NULL, tx TEXT NOT NULL, b BOOLEAN NOT NULL, j JSON NOT NULL,
    vb VARBINARY(10) NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

### Aggregates

```sql
SELECT SUM(i) AS sum_exact, SUM(d) AS sum_approximate, BIT_AND(vb) AS bit_binary FROM t
-- rules: sum_exact: mysql.aggregate.sum-exact / mysql.aggregate.sum-exact
-- rules: sum_approximate: mysql.aggregate.sum-approximate / mysql.aggregate.sum-approximate
-- rules: bit_binary: mysql.aggregate.bit-binary / mysql.aggregate.bit-binary
```

### Operators and functions

```sql
SELECT i / n AS division, tx / 1 AS division_other, i % tx AS modulo_other,
    i DIV 2 AS integer_division, ui + i AS unsigned, i + ui AS unsigned_right,
    i & i AS bitwise, i <=> i AS null_safe_equal, b XOR b AS xor, tx || tx AS "or",
    j -> '$.a' AS arrow_json, tx -> '$.a' AS arrow, UPPER(vb) AS binary_string,
    ROUND(i) AS round_integer, ROUND(n) AS round_decimal, ROUND(d) AS round_other
FROM t
-- rules: division: mysql.arithmetic.division / mysql.arithmetic.division
-- rules: division_other: mysql.arithmetic.division-other / mysql.arithmetic.division-other
-- rules: modulo_other: common.arithmetic.division-other / common.arithmetic.division-other
-- rules: integer_division: mysql.arithmetic.integer-division / mysql.arithmetic.integer-division
-- rules: unsigned: mysql.arithmetic.unsigned / mysql.arithmetic.unsigned
-- rules: unsigned_right: mysql.arithmetic.unsigned-right / mysql.arithmetic.unsigned-right
-- rules: bitwise: mysql.bitwise.numbers / mysql.bitwise.numbers
-- rules: null_safe_equal: mysql.comparison.null-safe-equal / mysql.comparison.null-safe-equal
-- rules: xor: common.logical.xor / common.logical.xor
-- rules: or: mysql.concat.or / mysql.concat.or
-- rules: arrow_json: mysql.json.arrow-json / mysql.json.arrow-json
-- rules: arrow: mysql.json.arrow / mysql.json.arrow
-- rules: binary_string: mysql.string.binary / mysql.string.binary
-- rules: round_integer: mysql.numeric.round-integer / mysql.numeric.round-integer
-- rules: round_decimal: mysql.numeric.round-decimal / mysql.numeric.round-decimal
-- rules: round_other: mysql.numeric.round-other / mysql.numeric.round-other
```

## SQLite

`schema.sql`:

```sql
CREATE TABLE t (i INTEGER NOT NULL, tx TEXT NOT NULL);
```

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

### Aggregates

```sql
SELECT SUM(i) AS sum FROM t
-- rules: sum: common.aggregate.sum / common.aggregate.sum
```

### Operators and functions

```sql
SELECT i & i AS bitwise, i / i AS division, tx -> '$.a' AS arrow, tx ->> '$.a' AS long_arrow,
    json_extract(tx, '$.a') AS extract, tx GLOB 'a*' AS glob
FROM t
-- rules: bitwise: sqlite.bitwise / sqlite.bitwise
-- rules: division: common.arithmetic.division / common.arithmetic.division
-- rules: arrow: sqlite.json.arrow / sqlite.json.arrow
-- rules: long_arrow: sqlite.json.long-arrow / sqlite.json.long-arrow
-- rules: extract: sqlite.json.extract / sqlite.json.extract
-- rules: glob: common.pattern / common.pattern
```

## Other dialects

The rules of `common/` that every dialect above overrides apply in the others.

`schema.sql`:

```sql
CREATE TABLE t (j JSON NOT NULL, jb JSONB NOT NULL);
```

```toml
[tool.sqlty]
dialect = "generic"
schema = "schema.sql"
```

```sql
SELECT j -> 'a' AS arrow, jb || jb AS jsonb, '{}' || jb AS jsonb_right FROM t
-- rules: arrow: common.json.arrow / common.json.arrow
-- rules: jsonb: common.concat.jsonb / common.concat.jsonb
-- rules: jsonb_right: common.concat.jsonb-right-operand / common.concat.jsonb-right-operand
```
