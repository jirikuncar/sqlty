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
    cn CIDR NOT NULL, rg INT4RANGE NOT NULL, tq TSQUERY NOT NULL, tv TSVECTOR NOT NULL,
    pt PATH NOT NULL, x XID NOT NULL
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
    rg + rg AS set_operations, ~i4 AS "not", @ i4 AS absolute
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
-- rules: not: postgres.bitwise.not / postgres.bitwise.not
-- rules: absolute: postgres.math.absolute / postgres.math.absolute
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

### Text search and geometry

```sql
SELECT tq && tq AS tq_and, tq || tq AS tq_or, tv || tv AS tv_concat, tq <-> tq AS tq_phrase,
    length(pt) AS path_length, ts_headline(jb, tq) AS headline
FROM t
-- rules: tq_and: postgres.textsearch.and / postgres.textsearch.and
-- rules: tq_or: postgres.textsearch.or / postgres.textsearch.or
-- rules: tv_concat: postgres.textsearch.concat-vectors / postgres.textsearch.concat-vectors
-- rules: tq_phrase: postgres.textsearch.followed-by / postgres.textsearch.followed-by
-- rules: path_length: postgres.geometry.length / postgres.geometry.length
-- rules: headline: postgres.textsearch.headline-json / postgres.textsearch.headline-json
```

### Money, log locations, bit strings and shapes

```sql
SELECT 1::money + 2::money AS amount, 1::money / 2::money AS ratio, 1::money * 2 AS scaled, 2 * 1::money AS scaled_right, '1/0'::pg_lsn - '0/1'::pg_lsn AS lsn_difference, '1/0'::pg_lsn + 1 AS lsn_offset, 1 + '1/0'::pg_lsn AS lsn_offset_left, B'10' & B'01' AS bits, B'10' << 1 AS bits_shift, B'10' || B'01' AS bits_concat, '08:00:2b:01:02:03'::macaddr & '08:00:2b:01:02:03'::macaddr AS mac, rg << rg AS strictly_left, '{[1,2)}'::int4multirange + '{[3,4)}'::int4multirange AS multirange_set, |/ d AS roots, !! tq AS tq_negate, 'a' ~<~ tx AS pattern FROM t
-- rules: amount: postgres.money.amount / postgres.money.amount
-- rules: ratio: postgres.money.ratio / postgres.money.ratio
-- rules: scaled: postgres.money.scaled / postgres.money.scaled
-- rules: scaled_right: postgres.money.scaled-right / postgres.money.scaled-right
-- rules: lsn_difference: postgres.lsn.difference / postgres.lsn.difference
-- rules: lsn_offset: postgres.lsn.offset / postgres.lsn.offset
-- rules: lsn_offset_left: postgres.lsn.offset-left / postgres.lsn.offset-left
-- rules: bits: postgres.bitwise.bits / postgres.bitwise.bits
-- rules: bits_shift: postgres.bitwise.bits-shift / postgres.bitwise.bits-shift
-- rules: bits_concat: postgres.concat.bits / postgres.concat.bits
-- rules: mac: postgres.bitwise.mac-addresses / postgres.bitwise.mac-addresses
-- rules: strictly_left: postgres.range.strictly-left-right / postgres.range.strictly-left-right
-- rules: multirange_set: postgres.range.multirange-set-operations / postgres.range.multirange-set-operations
-- rules: roots: postgres.math.roots / postgres.math.roots
-- rules: tq_negate: postgres.textsearch.negate / postgres.textsearch.negate
-- rules: pattern: postgres.comparison.pattern / postgres.comparison.pattern
```

```sql
SELECT pt + pt AS path_concat, '(1,1)'::point + '(1,1)'::point AS translate, '(1,1),(0,0)'::box << '(3,3),(2,2)'::box AS side, '(1,1)'::point <-> '(2,2)'::point AS distance, '(1,1)'::point ~= '(1,1)'::point AS predicates, '(1,1),(0,0)'::box # '(2,2),(0,0)'::box AS box_intersection, '[(0,0),(1,1)]'::lseg # '[(0,1),(1,0)]'::lseg AS intersection_point, '(1,1)'::point ## '[(0,0),(1,0)]'::lseg AS closest_point, @@ '(1,1),(0,0)'::box AS center, @-@ pt AS path_length_operator, # pt AS point_count, ?- '[(0,0),(1,0)]'::lseg AS orientation FROM t
-- rules: path_concat: postgres.geometry.path-concatenation / postgres.geometry.path-concatenation
-- rules: translate: postgres.geometry.translate / postgres.geometry.translate
-- rules: side: postgres.geometry.side / postgres.geometry.side
-- rules: distance: postgres.geometry.distance / postgres.geometry.distance
-- rules: predicates: postgres.geometry.predicates / postgres.geometry.predicates
-- rules: box_intersection: postgres.geometry.box-intersection / postgres.geometry.box-intersection
-- rules: intersection_point: postgres.geometry.intersection-point / postgres.geometry.intersection-point
-- rules: closest_point: postgres.geometry.closest-point / postgres.geometry.closest-point
-- rules: center: postgres.geometry.center / postgres.geometry.center
-- rules: path_length_operator: postgres.geometry.length-operator / postgres.geometry.length-operator
-- rules: point_count: postgres.geometry.point-count / postgres.geometry.point-count
-- rules: orientation: postgres.geometry.orientation / postgres.geometry.orientation
```

### Argument types

```sql
SELECT ts_headline('english', jb, tq) AS headline, string_agg(bt, bt) AS bytes, age(x) AS xid_age
FROM t
-- rules: headline: postgres.textsearch.headline-json-config / postgres.textsearch.headline-json-config
-- rules: bytes: postgres.aggregate.string-agg-bytea / postgres.aggregate.string-agg-bytea
-- rules: xid_age: postgres.system.age-xid / postgres.system.age-xid
```

### Built-in functions

Each built-in function a rule types, one call per rule.

```sql
SELECT
    pg_database_size(NULL::oid) AS admin_bigint,
    pg_try_advisory_lock(NULL::bigint) AS admin_boolean,
    pg_column_size(NULL::int4) AS admin_int,
    current_setting('datestyle') AS admin_text,
    pg_advisory_lock(NULL::bigint) AS admin_void
-- rules: admin_bigint: postgres.admin.bigint / postgres.admin.bigint
-- rules: admin_boolean: postgres.admin.boolean / postgres.admin.boolean
-- rules: admin_int: postgres.admin.int / postgres.admin.int
-- rules: admin_text: postgres.admin.text / postgres.admin.text
-- rules: admin_void: postgres.admin.void / postgres.admin.void
```

```sql
SELECT
    bit_and(NULL::bit) AS aggregate_argument_type,
    array_agg(NULL::int4) AS aggregate_array,
    stddev(NULL::real) AS aggregate_mean,
    any_value(NULL::int4) AS aggregate_argument,
    regr_count(NULL::double precision, NULL::double precision) AS aggregate_bigint,
    bool_and(NULL::boolean) AS aggregate_boolean,
    percent_rank() OVER () AS aggregate_double,
    corr(NULL::double precision, NULL::double precision) AS aggregate_aggregate_double
-- rules: aggregate_argument_type: postgres.aggregate.argument-type / postgres.aggregate.argument-type
-- rules: aggregate_array: postgres.aggregate.array / postgres.aggregate.array
-- rules: aggregate_mean: postgres.aggregate.mean / postgres.aggregate.mean
-- rules: aggregate_argument: postgres.aggregate.argument / postgres.aggregate.argument
-- rules: aggregate_bigint: postgres.aggregate.bigint / postgres.aggregate.bigint
-- rules: aggregate_boolean: postgres.aggregate.boolean / postgres.aggregate.boolean
-- rules: aggregate_double: common.window.distribution / common.window.distribution
-- rules: aggregate_aggregate_double: postgres.aggregate.aggregate-double / postgres.aggregate.aggregate-double
```

```sql
SELECT json_arrayagg(v) AS aggregate_json FROM (VALUES(2),(1)) t(v)
-- rules: aggregate_json: postgres.aggregate.json / postgres.aggregate.json
```

```sql
SELECT grouping(i4) AS aggregate_int FROM t GROUP BY i4
-- rules: aggregate_int: postgres.aggregate.int / postgres.aggregate.int
```

```sql
SELECT
    array_append(ARRAY[1,2], 3) AS array_argument_type,
    array_prepend(1, ARRAY[2,3]) AS array_argument_type_1,
    array_fill(11, ARRAY[2,3]) AS array_array,
    unnest(ARRAY[1,2]) AS array_element,
    array_positions(ARRAY['A','A','B','A'], 'A') AS array_int_array,
    array_length(array[1,2,3], 1) AS array_int,
    array_to_string(ARRAY[1, 2, 3, NULL, 5], ',', '*') AS array_text
-- rules: array_argument_type: postgres.array.argument-type-non-null / postgres.array.argument-type-non-null
-- rules: array_argument_type_1: postgres.array.argument-type-1 / postgres.array.argument-type-1
-- rules: array_array: postgres.array.array / postgres.array.array
-- rules: array_element: postgres.array.element / postgres.array.element
-- rules: array_int_array: postgres.array.int-array / postgres.array.int-array
-- rules: array_int: postgres.array.int-nullable / postgres.array.int-nullable
-- rules: array_text: postgres.array.text / postgres.array.text
```

```sql
SELECT
    set_byte('\x1234567890'::bytea, 4, 64) AS binary_argument_type,
    bit_count(B'10111') AS binary_bigint,
    get_byte('\x1234567890'::bytea, 4) AS binary_int,
    encode('123\000\001', 'base64') AS binary_text,
    sha224('abc'::bytea) AS binary_varbinary
-- rules: binary_argument_type: postgres.binary.argument-type / postgres.binary.argument-type
-- rules: binary_bigint: postgres.binary.bigint / postgres.binary.bigint
-- rules: binary_int: postgres.binary.int / postgres.binary.int
-- rules: binary_text: postgres.binary.text / postgres.binary.text
-- rules: binary_varbinary: postgres.binary.varbinary / postgres.binary.varbinary
```

```sql
SELECT
    rank() OVER () AS aggregate_bigint,
    json_agg(NULL::int4) AS aggregate_json,
    jsonb_agg(NULL::int4) AS aggregate_jsonb,
    string_agg(NULL::text, NULL::text) AS aggregate_text,
    coalesce(1) AS conditional_common,
    current_date AS datetime_date,
    now() AS datetime_timestamptz,
    current_time(2) AS datetime_timetz,
    json_build_object('foo', 1, 2, row(3,'bar')) AS json_json,
    jsonb_build_object() AS json_jsonb,
    length('jose') AS string_int,
    concat('abcde', 2, NULL, 22) AS string_text,
    row_number() OVER () AS window_bigint,
    ntile(NULL::integer) OVER () AS window_int
-- rules: aggregate_bigint: common.aggregate.bigint / common.aggregate.bigint
-- rules: aggregate_json: common.aggregate.json / common.aggregate.json
-- rules: aggregate_jsonb: common.aggregate.jsonb / common.aggregate.jsonb
-- rules: aggregate_text: common.aggregate.text / common.aggregate.text
-- rules: conditional_common: common.conditional.common / common.conditional.common
-- rules: datetime_date: common.datetime.date / common.datetime.date
-- rules: datetime_timestamptz: common.datetime.timestamptz / common.datetime.timestamptz
-- rules: datetime_timetz: common.datetime.timetz / common.datetime.timetz
-- rules: json_json: common.json.json-non-null / common.json.json-non-null
-- rules: json_jsonb: common.json.jsonb-non-null / common.json.jsonb-non-null
-- rules: string_int: common.string.int / common.string.int
-- rules: string_text: common.string.text-non-null / common.string.text-non-null
-- rules: window_bigint: common.window.bigint / common.window.bigint
-- rules: window_int: common.window.int / common.window.int
```

```sql
SELECT num_nonnulls(1, NULL, 2) AS comparison_int
-- rules: comparison_int: postgres.comparison.int / postgres.comparison.int
```

```sql
SELECT greatest(1) AS conditional_common, nullif('a', 'b') AS conditional_argument
-- rules: conditional_common: postgres.conditional.common / postgres.conditional.common
-- rules: conditional_argument: common.conditional.nullif / common.conditional.nullif
```

```sql
SELECT
    date_bin('15 minutes', timestamp '2001-02-16 20:38:40', timestamp '2001-02-16 20:05:00') AS datetime_argument_type_1,
    timezone(NULL::time with time zone) AS datetime_at_zone,
    isfinite(date '2001-02-16') AS datetime_boolean,
    make_date(2013, 7, 15) AS datetime_date,
    date_part('month', interval '2 years 3 months') AS datetime_double,
    justify_days(interval '1 year 65 days') AS datetime_interval,
    timeofday() AS datetime_text,
    localtime(0) AS datetime_time,
    localtimestamp(2) AS datetime_timestamp,
    transaction_timestamp() AS datetime_timestamptz,
    date_trunc('hour', timestamp '2001-02-16 20:38:40') AS datetime_truncated,
    pg_sleep(NULL::double precision) AS datetime_void
-- rules: datetime_argument_type_1: postgres.datetime.argument-type-1 / postgres.datetime.argument-type-1
-- rules: datetime_at_zone: postgres.datetime.at-zone / postgres.datetime.at-zone
-- rules: datetime_boolean: postgres.datetime.boolean / postgres.datetime.boolean
-- rules: datetime_date: postgres.datetime.date / postgres.datetime.date
-- rules: datetime_double: postgres.datetime.double / postgres.datetime.double
-- rules: datetime_interval: postgres.datetime.interval / postgres.datetime.interval
-- rules: datetime_text: postgres.datetime.text / postgres.datetime.text
-- rules: datetime_time: postgres.datetime.time-non-null / postgres.datetime.time-non-null
-- rules: datetime_timestamp: postgres.datetime.timestamp-non-null / postgres.datetime.timestamp-non-null
-- rules: datetime_timestamptz: postgres.datetime.timestamptz / postgres.datetime.timestamptz
-- rules: datetime_truncated: postgres.datetime.truncated / postgres.datetime.truncated
-- rules: datetime_void: postgres.datetime.void / postgres.datetime.void
```

```sql
SELECT uuid_generate_v4() AS extension_uuid
-- rules: extension_uuid: postgres.extension.uuid-non-null / postgres.extension.uuid-non-null
```

```sql
SELECT digest('abc', 'sha256') AS extension_varbinary
-- rules: extension_varbinary: postgres.extension.varbinary / postgres.extension.varbinary
```

```sql
SELECT
    to_date('05 Dec 2000', 'DD Mon YYYY') AS formatting_date,
    to_number('12,454.8-', '99G999D9S') AS formatting_decimal,
    to_char(125, '999') AS formatting_text
-- rules: formatting_date: postgres.formatting.date / postgres.formatting.date
-- rules: formatting_decimal: postgres.formatting.decimal / postgres.formatting.decimal
-- rules: formatting_text: postgres.formatting.text / postgres.formatting.text
```

```sql
SELECT
    jsonb_path_exists('{"a":[1,2,3,4,5]}', '$.a[*] ? (@ >= $min && @ <= $max)', '{"min":2, "max":4}') AS json_boolean,
    jsonb_array_length('[]') AS json_int,
    row_to_json(row(1,'foo')) AS json_json,
    json_query('{"a": 1}', '$.a') AS json_jsonb,
    json_each(NULL::json) AS json_record,
    jsonb_typeof(NULL::jsonb) AS json_text
-- rules: json_boolean: postgres.json.boolean / postgres.json.boolean
-- rules: json_int: postgres.json.int / postgres.json.int
-- rules: json_json: postgres.json.json / postgres.json.json
-- rules: json_jsonb: postgres.json.jsonb-nullable / postgres.json.jsonb-nullable
-- rules: json_record: postgres.json.record / postgres.json.record
-- rules: json_text: postgres.json.text / postgres.json.text
```

```sql
SELECT
    abs(-17.4) AS math_argument_type,
    random() AS math_random,
    ceiling(95.3) AS math_real,
    round(42.4) AS math_scaled,
    trim_scale(8.4100) AS math_decimal,
    cbrt(64.0) AS math_double,
    width_bucket(9, 10, 0, 10) AS math_int,
    setseed(NULL::double precision) AS math_void
-- rules: math_argument_type: common.math.argument-type / common.math.argument-type
-- rules: math_random: postgres.math.random / postgres.math.random
-- rules: math_real: postgres.math.real / postgres.math.real
-- rules: math_scaled: postgres.math.scaled / postgres.math.scaled
-- rules: math_decimal: postgres.math.decimal / postgres.math.decimal
-- rules: math_double: postgres.math.double / postgres.math.double
-- rules: math_int: postgres.math.int / postgres.math.int
-- rules: math_void: postgres.math.void / postgres.math.void
```

```sql
SELECT
    set_masklen(cidr '192.168.1.0/24', 16) AS network_argument_type,
    network(inet '192.168.1.5/24') AS network_cidr,
    broadcast(inet '192.168.1.5/24') AS network_inet,
    masklen(inet '192.168.1.5/24') AS network_int,
    host(inet '192.168.1.0/24') AS network_text
-- rules: network_argument_type: postgres.network.argument-type / postgres.network.argument-type
-- rules: network_cidr: postgres.network.cidr / postgres.network.cidr
-- rules: network_inet: postgres.network.inet / postgres.network.inet
-- rules: network_int: postgres.network.int / postgres.network.int
-- rules: network_text: postgres.network.text / postgres.network.text
```

```sql
SELECT
    range_merge('{[1,2), [3,4)}'::int4multirange) AS range_range,
    multirange(int4range(1, 2)) AS range_multirange,
    unnest('{[1,2), [3,4)}'::int4multirange) AS array_unnested_multirange,
    range_agg(int4range(1, 2)) AS aggregate_multirange,
    lower_inf('(,)'::daterange) AS range_boolean,
    int4range(1, 2) AS range_int4range,
    int8range(1, 2) AS range_int8range,
    numrange(1, 2) AS range_numrange,
    daterange('2024-01-01'::date, '2024-02-01'::date) AS range_daterange,
    tsrange('2024-01-01'::timestamp, '2024-02-01'::timestamp) AS range_tsrange,
    tstzrange('2024-01-01'::timestamptz, '2024-02-01'::timestamptz) AS range_tstzrange
-- rules: range_range: postgres.range.range / postgres.range.range
-- rules: range_multirange: postgres.range.multirange / postgres.range.multirange
-- rules: array_unnested_multirange: postgres.array.unnested-multirange / postgres.array.unnested-multirange
-- rules: aggregate_multirange: postgres.aggregate.multirange / postgres.aggregate.multirange
-- rules: range_boolean: postgres.range.boolean / postgres.range.boolean
-- rules: range_int4range: postgres.range.int4range / postgres.range.int4range
-- rules: range_int8range: postgres.range.int8range / postgres.range.int8range
-- rules: range_numrange: postgres.range.numrange / postgres.range.numrange
-- rules: range_daterange: postgres.range.daterange / postgres.range.daterange
-- rules: range_tsrange: postgres.range.tsrange / postgres.range.tsrange
-- rules: range_tstzrange: postgres.range.tstzrange / postgres.range.tstzrange
```

```sql
SELECT row() AS row_record
-- rules: row_record: postgres.row.record / postgres.row.record
```

```sql
SELECT nextval(NULL::regclass) AS sequence_bigint
-- rules: sequence_bigint: postgres.sequence.bigint / postgres.sequence.bigint
```

```sql
SELECT generate_series(NULL::bigint, NULL::bigint) AS series_series
-- rules: series_series: postgres.series.series / postgres.series.series
```

```sql
SELECT
    lower('TOM') AS string_bound,
    string_to_array('xx~~yy~~zz', '~~', 'yy') AS string_text_array,
    starts_with('alphabet', 'alph') AS string_boolean,
    ascii('x') AS string_int,
    pg_client_encoding() AS string_name,
    left('abcde', 2) AS string_text,
    ltrim('zzzytest', 'xyz') AS string_string
-- rules: string_bound: postgres.string.bound / postgres.string.bound
-- rules: string_text_array: postgres.string.text-array / postgres.string.text-array
-- rules: string_boolean: postgres.string.boolean / postgres.string.boolean
-- rules: string_int: postgres.string.int / postgres.string.int
-- rules: string_name: postgres.string.name / postgres.string.name
-- rules: string_text: postgres.string.text / postgres.string.text
-- rules: string_string: postgres.string.string / postgres.string.string
```

```sql
SELECT
    age(timestamp '1957-06-13') AS system_interval,
    txid_current() AS system_bigint,
    has_table_privilege(NULL::oid, NULL::text) AS system_boolean,
    inet_client_addr() AS system_inet,
    pg_backend_pid() AS system_int,
    current_database() AS system_name,
    pg_typeof(33) AS system_regtype,
    version() AS system_text,
    pg_postmaster_start_time() AS system_timestamptz,
    pg_notify('a', 'b') AS system_void
-- rules: system_interval: postgres.system.interval / postgres.system.interval
-- rules: system_bigint: postgres.system.bigint / postgres.system.bigint
-- rules: system_boolean: postgres.system.boolean / postgres.system.boolean
-- rules: system_inet: postgres.system.inet / postgres.system.inet
-- rules: system_int: postgres.system.int / postgres.system.int
-- rules: system_name: postgres.system.name / postgres.system.name
-- rules: system_regtype: postgres.system.regtype / postgres.system.regtype
-- rules: system_text: postgres.system.text / postgres.system.text
-- rules: system_timestamptz: postgres.system.timestamptz / postgres.system.timestamptz
-- rules: system_void: postgres.system.void / postgres.system.void
```

```sql
SELECT
    ts_rank(to_tsvector('raining cats and dogs'), 'cat') AS textsearch_float,
    ts_headline('The fat cat ate the rat.', 'cat') AS textsearch_text,
    to_tsquery('english', 'The & Fat & Rats') AS textsearch_tsquery,
    to_tsvector('english', 'The Fat Rats') AS textsearch_tsvector
-- rules: textsearch_float: postgres.textsearch.float / postgres.textsearch.float
-- rules: textsearch_text: postgres.textsearch.text / postgres.textsearch.text
-- rules: textsearch_tsquery: postgres.textsearch.tsquery / postgres.textsearch.tsquery
-- rules: textsearch_tsvector: postgres.textsearch.tsvector / postgres.textsearch.tsvector
```

```sql
SELECT
    uuid_extract_version(NULL::uuid) AS uuid_smallint,
    uuid_extract_timestamp(NULL::uuid) AS uuid_timestamptz,
    gen_random_uuid() AS uuid_uuid
-- rules: uuid_smallint: postgres.uuid.smallint / postgres.uuid.smallint
-- rules: uuid_timestamptz: postgres.uuid.timestamptz / postgres.uuid.timestamptz
-- rules: uuid_uuid: postgres.uuid.uuid / postgres.uuid.uuid
```

```sql
SELECT first_value(NULL::int4) OVER () AS window_argument
-- rules: window_argument: common.window.value / common.window.value
```

```sql
SELECT mod(7, 3) AS math_argument_type, sin(1.0) AS math_double
-- rules: math_argument_type: postgres.math.argument-type / postgres.math.argument-type
-- rules: math_double: common.math.double / common.math.double
```

```sql
SELECT
    array_remove(ARRAY[1,2,3,2], 2) AS array_argument_type,
    array_cat(ARRAY[1,2,3], ARRAY[4,5]) AS array_argument_type_all_arguments,
    array_ndims(ARRAY[[1,2,3], [4,5,6]]) AS array_int,
    array_dims(ARRAY[[1,2,3], [4,5,6]]) AS array_text_nullable
-- rules: array_argument_type: postgres.array.argument-type / postgres.array.argument-type
-- rules: array_argument_type_all_arguments: postgres.array.argument-type-all-arguments / postgres.array.argument-type-all-arguments
-- rules: array_int: postgres.array.int / postgres.array.int
-- rules: array_text_nullable: postgres.array.text-nullable / postgres.array.text-nullable
```

```sql
SELECT
    to_json('Fred said "Hi."'::text) AS json_json,
    to_jsonb(row(42, 'Fred said "Hi."'::text)) AS json_jsonb,
    replace('abcdefabcdef', 'cd', 'XX') AS string_text,
    nth_value(NULL::int4, NULL::integer) OVER () AS window_value_nullable
-- rules: json_json: common.json.json / common.json.json
-- rules: json_jsonb: common.json.jsonb / common.json.jsonb
-- rules: string_text: common.string.text / common.string.text
-- rules: window_value_nullable: common.window.value-nullable / common.window.value-nullable
```

```sql
SELECT
    make_time(8, 15, 23.5) AS datetime_time,
    make_timestamp(2013, 7, 15, 8, 15, 23.5) AS datetime_timestamp
-- rules: datetime_time: postgres.datetime.time / postgres.datetime.time
-- rules: datetime_timestamp: postgres.datetime.timestamp / postgres.datetime.timestamp
```

```sql
SELECT uuid_generate_v1() AS extension_uuid
-- rules: extension_uuid: postgres.extension.uuid / postgres.extension.uuid
```

```sql
SELECT
    jsonb_path_match('{"a":[1,2,3,4,5]}', 'exists($.a[*] ? (@ >= $min && @ <= $max))', '{"min":2, "max":4}') AS json_boolean_nullable,
    json_build_array(1, 2, 'foo', 4, 5) AS json_json_non_null,
    json_extract_path('{"f2":{"f3":1},"f4":{"f5":99,"f6":"foo"}}', 'f4', 'f6') AS json_json_nullable,
    jsonb_object(NULL::text[]) AS json_jsonb,
    jsonb_build_array() AS json_jsonb_non_null,
    json_extract_path_text('{"f2":{"f3":1},"f4":{"f5":99,"f6":"foo"}}', 'f4', 'f6') AS json_text_nullable
-- rules: json_boolean_nullable: postgres.json.boolean-nullable / postgres.json.boolean-nullable
-- rules: json_json_non_null: postgres.json.json-non-null / postgres.json.json-non-null
-- rules: json_json_nullable: postgres.json.json-nullable / postgres.json.json-nullable
-- rules: json_jsonb: postgres.json.jsonb / postgres.json.jsonb
-- rules: json_jsonb_non_null: postgres.json.jsonb-non-null / postgres.json.jsonb-non-null
-- rules: json_text_nullable: postgres.json.text-nullable / postgres.json.text-nullable
```

```sql
SELECT
    regexp_match('foobarbequebaz', '(bar)(beque)') AS string_text_array_nullable,
    quote_nullable(NULL) AS string_text_non_null,
    regexp_substr('ABCDEF', 'c(.)(..)', 1, 1, 'i') AS string_text_nullable
-- rules: string_text_array_nullable: postgres.string.text-array-nullable / postgres.string.text-array-nullable
-- rules: string_text_non_null: postgres.string.text-non-null / postgres.string.text-non-null
-- rules: string_text_nullable: postgres.string.text-nullable / postgres.string.text-nullable
```

```sql
SELECT inet_client_port() AS system_int_nullable
-- rules: system_int_nullable: postgres.system.int-nullable / postgres.system.int-nullable
```

```sql
SELECT
    gin_clean_pending_list(NULL::regclass) AS admin_bigint_nullable,
    pg_is_wal_replay_paused() AS admin_boolean_nullable,
    pg_wal_lsn_diff(NULL::pg_lsn, NULL::pg_lsn) AS admin_decimal,
    brin_summarize_new_values(NULL::regclass) AS admin_int_nullable,
    pg_column_toast_chunk_id(NULL::int4) AS admin_oid_nullable,
    pg_backup_start(NULL::text, NULL::boolean) AS admin_pg_lsn_nullable,
    pg_backup_stop(NULL::boolean) AS admin_record_nullable,
    pg_filenode_relation(NULL::oid, NULL::oid) AS admin_regclass_nullable,
    pg_collation_actual_version(NULL::oid) AS admin_text_nullable,
    pg_last_xact_replay_timestamp() AS admin_timestamptz_nullable,
    pg_read_binary_file(NULL::text) AS admin_varbinary_nullable
-- rules: admin_bigint_nullable: postgres.admin.bigint-nullable / postgres.admin.bigint-nullable
-- rules: admin_boolean_nullable: postgres.admin.boolean-nullable / postgres.admin.boolean-nullable
-- rules: admin_decimal: postgres.admin.decimal / postgres.admin.decimal
-- rules: admin_int_nullable: postgres.admin.int-nullable / postgres.admin.int-nullable
-- rules: admin_oid_nullable: postgres.admin.oid-nullable / postgres.admin.oid-nullable
-- rules: admin_pg_lsn_nullable: postgres.admin.pg-lsn-nullable / postgres.admin.pg-lsn-nullable
-- rules: admin_record_nullable: postgres.admin.record-nullable / postgres.admin.record-nullable
-- rules: admin_regclass_nullable: postgres.admin.regclass-nullable / postgres.admin.regclass-nullable
-- rules: admin_text_nullable: postgres.admin.text-nullable / postgres.admin.text-nullable
-- rules: admin_timestamptz_nullable: postgres.admin.timestamptz-nullable / postgres.admin.timestamptz-nullable
-- rules: admin_varbinary_nullable: postgres.admin.varbinary-nullable / postgres.admin.varbinary-nullable
```

```sql
SELECT
    json_agg_strict(NULL::int4) AS aggregate_json_strict,
    jsonb_agg_strict(NULL::int4) AS aggregate_jsonb_strict,
    xmlagg(NULL::xml) AS aggregate_xml
-- rules: aggregate_json_strict: postgres.aggregate.json-strict / postgres.aggregate.json-strict
-- rules: aggregate_jsonb_strict: postgres.aggregate.jsonb-strict / postgres.aggregate.jsonb-strict
-- rules: aggregate_xml: postgres.aggregate.xml / postgres.aggregate.xml
```

```sql
SELECT
    pg_event_trigger_table_rewrite_reason() AS event_int,
    pg_event_trigger_table_rewrite_oid() AS event_oid
-- rules: event_int: postgres.event.int / postgres.event.int
-- rules: event_oid: postgres.event.oid / postgres.event.oid
```

```sql
SELECT
    isclosed(path '((0,0),(1,1),(2,0))') AS geometry_boolean,
    bound_box(box '(1,1),(0,0)', box '(4,4),(3,3)') AS geometry_box,
    circle(box '(1,1),(0,0)') AS geometry_circle,
    area(box '(2,2),(0,0)') AS geometry_double,
    npoints(path '[(0,0),(1,1),(2,0)]') AS geometry_int,
    line(point '(-1,0)', point '(1,0)') AS geometry_line,
    diagonal(box '(1,2),(0,0)') AS geometry_lseg,
    path(polygon '((0,0),(1,1),(2,0))') AS geometry_path,
    center(box '(1,2),(0,0)') AS geometry_point,
    polygon(box '(1,1),(0,0)') AS geometry_polygon
-- rules: geometry_boolean: postgres.geometry.boolean / postgres.geometry.boolean
-- rules: geometry_box: postgres.geometry.box / postgres.geometry.box
-- rules: geometry_circle: postgres.geometry.circle / postgres.geometry.circle
-- rules: geometry_double: postgres.geometry.double / postgres.geometry.double
-- rules: geometry_int: postgres.geometry.int / postgres.geometry.int
-- rules: geometry_line: postgres.geometry.line / postgres.geometry.line
-- rules: geometry_lseg: postgres.geometry.lseg / postgres.geometry.lseg
-- rules: geometry_path: postgres.geometry.path / postgres.geometry.path
-- rules: geometry_point: postgres.geometry.point / postgres.geometry.point
-- rules: geometry_polygon: postgres.geometry.polygon / postgres.geometry.polygon
```

```sql
SELECT
    jsonb_populate_record_valid(NULL::int4, NULL::jsonb) AS json_populate_record_valid,
    jsonb_set_lax('[{"f1":1,"f2":null},2,null,3]', '{0,f1}', null) AS json_set_lax,
    json_populate_record(NULL::int4, NULL::json, NULL::boolean) AS json_populated
-- rules: json_populate_record_valid: postgres.json.populate-record-valid / postgres.json.populate-record-valid
-- rules: json_set_lax: postgres.json.set-lax / postgres.json.set-lax
-- rules: json_populated: postgres.json.populated / postgres.json.populated
```

```sql
SELECT
    trunc(macaddr '12:34:56:78:90:ab') AS math_truncated_address,
    gcd(1071, 462) AS math_common_integer,
    lower(int4range(1, 2)) AS string_range_bound,
    unnest('cat:3 fat:2,4'::tsvector) AS array_unnested_tsvector
-- rules: math_truncated_address: postgres.math.truncated-address / postgres.math.truncated-address
-- rules: math_common_integer: postgres.math.common-integer / postgres.math.common-integer
-- rules: string_range_bound: postgres.string.range-bound / postgres.string.range-bound
-- rules: array_unnested_tsvector: postgres.array.unnested-tsvector / postgres.array.unnested-tsvector
```

```sql
SELECT
    inet_same_family(inet '192.168.1.5/24', inet '::1') AS network_boolean,
    macaddr8_set7bit(macaddr8 '00:34:56:ab:cd:ef') AS network_macaddr8
-- rules: network_boolean: postgres.network.boolean / postgres.network.boolean
-- rules: network_macaddr8: postgres.network.macaddr8 / postgres.network.macaddr8
```

```sql
SELECT generate_subscripts(NULL::int4[], NULL::integer) AS series_int
-- rules: series_int: postgres.series.int / postgres.series.int
```

```sql
SELECT
    makeaclitem(NULL::oid, NULL::oid, NULL::text, NULL::boolean) AS system_aclitem,
    acldefault(NULL::"char", NULL::oid) AS system_aclitem_array,
    pg_blocking_pids(NULL::integer) AS system_int_array,
    current_schemas(NULL::boolean) AS system_name_array,
    pg_settings_get_flags(NULL::text) AS system_text_array_nullable,
    txid_current_if_assigned() AS system_bigint_nullable,
    pg_collation_is_visible(NULL::oid) AS system_boolean_nullable,
    pg_notification_queue_usage() AS system_double,
    to_regtypemod(NULL::text) AS system_type_modifier,
    pg_my_temp_schema() AS system_oid_nullable,
    pg_current_snapshot() AS system_pg_snapshot,
    aclexplode(NULL::aclitem[]) AS system_record_nullable,
    to_regclass(NULL::text) AS system_regclass_nullable,
    to_regcollation(NULL::text) AS system_regcollation_nullable,
    to_regnamespace(NULL::text) AS system_regnamespace_nullable,
    to_regoper(NULL::text) AS system_regoper_nullable,
    to_regoperator(NULL::text) AS system_regoperator_nullable,
    to_regproc(NULL::text) AS system_regproc_nullable,
    to_regprocedure(NULL::text) AS system_regprocedure_nullable,
    to_regrole(NULL::text) AS system_regrole_nullable,
    pg_basetype(NULL::regtype) AS system_regtype_nullable,
    current_query() AS system_text_nullable,
    pg_xact_commit_timestamp(NULL::xid) AS system_timestamptz_nullable,
    txid_current_snapshot() AS system_txid_snapshot,
    pg_current_xact_id_if_assigned() AS system_xid8_nullable,
    pg_current_xact_id() AS system_xid8
-- rules: system_aclitem: postgres.system.aclitem / postgres.system.aclitem
-- rules: system_aclitem_array: postgres.system.aclitem-array / postgres.system.aclitem-array
-- rules: system_int_array: postgres.system.int-array / postgres.system.int-array
-- rules: system_name_array: postgres.system.name-array / postgres.system.name-array
-- rules: system_text_array_nullable: postgres.system.text-array-nullable / postgres.system.text-array-nullable
-- rules: system_bigint_nullable: postgres.system.bigint-nullable / postgres.system.bigint-nullable
-- rules: system_boolean_nullable: postgres.system.boolean-nullable / postgres.system.boolean-nullable
-- rules: system_double: postgres.system.double / postgres.system.double
-- rules: system_type_modifier: postgres.system.type-modifier / postgres.system.type-modifier
-- rules: system_oid_nullable: postgres.system.oid-nullable / postgres.system.oid-nullable
-- rules: system_pg_snapshot: postgres.system.pg-snapshot / postgres.system.pg-snapshot
-- rules: system_record_nullable: postgres.system.record-nullable / postgres.system.record-nullable
-- rules: system_regclass_nullable: postgres.system.regclass-nullable / postgres.system.regclass-nullable
-- rules: system_regcollation_nullable: postgres.system.regcollation-nullable / postgres.system.regcollation-nullable
-- rules: system_regnamespace_nullable: postgres.system.regnamespace-nullable / postgres.system.regnamespace-nullable
-- rules: system_regoper_nullable: postgres.system.regoper-nullable / postgres.system.regoper-nullable
-- rules: system_regoperator_nullable: postgres.system.regoperator-nullable / postgres.system.regoperator-nullable
-- rules: system_regproc_nullable: postgres.system.regproc-nullable / postgres.system.regproc-nullable
-- rules: system_regprocedure_nullable: postgres.system.regprocedure-nullable / postgres.system.regprocedure-nullable
-- rules: system_regrole_nullable: postgres.system.regrole-nullable / postgres.system.regrole-nullable
-- rules: system_regtype_nullable: postgres.system.regtype-nullable / postgres.system.regtype-nullable
-- rules: system_text_nullable: postgres.system.text-nullable / postgres.system.text-nullable
-- rules: system_timestamptz_nullable: postgres.system.timestamptz-nullable / postgres.system.timestamptz-nullable
-- rules: system_txid_snapshot: postgres.system.txid-snapshot / postgres.system.txid-snapshot
-- rules: system_xid8_nullable: postgres.system.xid8-nullable / postgres.system.xid8-nullable
-- rules: system_xid8: postgres.system.xid8 / postgres.system.xid8
```

```sql
SELECT
    ts_lexize('english_stem', 'stars') AS textsearch_text_array_nullable,
    tsvector_to_array('fat:2,4 cat:3 rat:5A'::tsvector) AS textsearch_text_array,
    numnode('(fat & rat) | cat'::tsquery) AS textsearch_int,
    ts_debug('english', 'The Brightest supernovaes') AS textsearch_record,
    get_current_ts_config() AS textsearch_regconfig
-- rules: textsearch_text_array_nullable: postgres.textsearch.text-array-nullable / postgres.textsearch.text-array-nullable
-- rules: textsearch_text_array: postgres.textsearch.text-array / postgres.textsearch.text-array
-- rules: textsearch_int: postgres.textsearch.int / postgres.textsearch.int
-- rules: textsearch_record: postgres.textsearch.record / postgres.textsearch.record
-- rules: textsearch_regconfig: postgres.textsearch.regconfig / postgres.textsearch.regconfig
```

```sql
SELECT
    xpath(NULL::text, NULL::xml) AS xml_xml_array,
    xml_is_well_formed(NULL::text) AS xml_boolean,
    cursor_to_xml(NULL::refcursor, NULL::integer, NULL::boolean, NULL::boolean, NULL::text) AS xml_xml
-- rules: xml_xml_array: postgres.xml.xml-array / postgres.xml.xml-array
-- rules: xml_boolean: postgres.xml.boolean / postgres.xml.boolean
-- rules: xml_xml: postgres.xml.xml / postgres.xml.xml
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
    CONCAT(tx, vb) AS binary_any_argument, REGEXP_SUBSTR(tx, vb) AS binary_pattern,
    ROUND(i) AS round_integer, ROUND(n) AS round_decimal, ROUND(d) AS round_other, ~i AS "not"
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
-- rules: binary_any_argument: mysql.string.binary-any-argument / mysql.string.binary-any-argument
-- rules: binary_pattern: mysql.string.binary-pattern-nullable / mysql.string.binary-pattern-nullable
-- rules: round_integer: mysql.numeric.round-integer / mysql.numeric.round-integer
-- rules: round_decimal: mysql.numeric.round-decimal / mysql.numeric.round-decimal
-- rules: round_other: mysql.numeric.round-other / mysql.numeric.round-other
-- rules: not: mysql.bitwise.not / mysql.bitwise.not
```

### Built-in functions

Each built-in function a rule types, one call per rule.

```sql
SELECT std(1) AS aggregate_double, json_arrayagg(1) AS aggregate_json
-- rules: aggregate_double: mysql.aggregate.double / mysql.aggregate.double
-- rules: aggregate_json: mysql.aggregate.json / mysql.aggregate.json
```

```sql
SELECT BIT_COUNT(127) AS bits_bigint
-- rules: bits_bigint: mysql.bits.bigint / mysql.bits.bigint
```

```sql
SELECT stddev(1) AS aggregate_double
-- rules: aggregate_double: common.aggregate.double / common.aggregate.double
```

```sql
SELECT GREATEST(2,0) AS comparison_common, ISNULL(1+1) AS comparison_bigint
-- rules: comparison_common: mysql.comparison.common / mysql.comparison.common
-- rules: comparison_bigint: mysql.comparison.bigint / mysql.comparison.bigint
```

```sql
SELECT
    IF(1>2,2,3) AS conditional_branches,
    IFNULL(1,0) AS conditional_common,
    NULLIF(1,2) AS conditional_argument
-- rules: conditional_branches: mysql.conditional.branches / mysql.conditional.branches
-- rules: conditional_common: mysql.conditional.common / mysql.conditional.common
-- rules: conditional_argument: common.conditional.nullif / common.conditional.nullif
```

```sql
SELECT
    ADDTIME('01:00:00.999999', '02:00:00.999998') AS datetime_argument_type,
    DATE_ADD('2006-05-00',INTERVAL 1 DAY) AS datetime_date_add,
    UNIX_TIMESTAMP() AS datetime_bigint,
    CURDATE() AS datetime_date,
    NOW() AS datetime_datetime,
    DATE_FORMAT('2006-06-00', '%d') AS datetime_text,
    CURTIME() AS datetime_time,
    STR_TO_DATE('9','%m') AS datetime_format,
    TIMESTAMPADD(WEEK,1,'2003-01-02') AS datetime_timestamp_add
-- rules: datetime_argument_type: mysql.datetime.argument-type / mysql.datetime.argument-type
-- rules: datetime_date_add: mysql.datetime.date-add / mysql.datetime.date-add
-- rules: datetime_bigint: mysql.datetime.bigint / mysql.datetime.bigint
-- rules: datetime_date: mysql.datetime.date / mysql.datetime.date
-- rules: datetime_datetime: mysql.datetime.datetime-non-null / mysql.datetime.datetime-non-null
-- rules: datetime_text: mysql.datetime.text / mysql.datetime.text
-- rules: datetime_time: mysql.datetime.time / mysql.datetime.time
-- rules: datetime_format: mysql.datetime.format / mysql.datetime.format
-- rules: datetime_timestamp_add: mysql.datetime.timestamp-add / mysql.datetime.timestamp-add
```

```sql
SELECT MD5('testing') AS encryption_text, compress(1) AS encryption_varbinary
-- rules: encryption_text: mysql.encryption.text / mysql.encryption.text
-- rules: encryption_varbinary: mysql.encryption.varbinary / mysql.encryption.varbinary
```

```sql
SELECT
    JSON_LENGTH('[1, 2, {"a": 3}]') AS json_bigint,
    JSON_SCHEMA_VALID('{"type":"string","pattern":"("}', '"abc"') AS json_int,
    JSON_OBJECT('id', 87, 'name', 'carrot') AS json_json,
    JSON_UNQUOTE('"abc"') AS json_text
-- rules: json_bigint: mysql.json.bigint / mysql.json.bigint
-- rules: json_int: mysql.json.int / mysql.json.int
-- rules: json_json: mysql.json.json-non-null / mysql.json.json-non-null
-- rules: json_text: mysql.json.text / mysql.json.text
```

```sql
SELECT
    ABS(2) AS math_argument_type,
    CEILING(1.23) AS math_integral,
    CRC32('mysql') AS math_bigint,
    SQRT(4) AS math_double,
    CONV('a',16,2) AS math_text
-- rules: math_argument_type: common.math.argument-type / common.math.argument-type
-- rules: math_integral: mysql.math.integral / mysql.math.integral
-- rules: math_bigint: mysql.math.bigint / mysql.math.bigint
-- rules: math_double: mysql.math.double-nullable / mysql.math.double-nullable
-- rules: math_text: mysql.math.text / mysql.math.text
```

```sql
SELECT
    any_value(1) AS misc_argument,
    SLEEP(0) AS misc_bigint,
    is_ipv4(1) AS misc_int,
    INET_NTOA(167773449) AS misc_text,
    UUID_SHORT() AS misc_ubigint,
    uuid_to_bin(UUID()) AS misc_varbinary
-- rules: misc_argument: mysql.misc.argument / mysql.misc.argument
-- rules: misc_bigint: mysql.misc.bigint / mysql.misc.bigint
-- rules: misc_int: mysql.misc.int / mysql.misc.int
-- rules: misc_text: mysql.misc.text-nullable / mysql.misc.text-nullable
-- rules: misc_ubigint: mysql.misc.ubigint / mysql.misc.ubigint
-- rules: misc_varbinary: mysql.misc.varbinary / mysql.misc.varbinary
```

```sql
SELECT ST_Distance(ST_GeomFromText('POINT(1 1)'), ST_GeomFromText('POINT(2 2)')) AS spatial_double
-- rules: spatial_double: mysql.spatial.double / mysql.spatial.double
```

```sql
SELECT
    bit_and(1) AS string_bigint,
    round('a') AS string_argument_type,
    REGEXP_INSTR('dog cat dog', 'dog') AS string_bigint_2,
    BIN(12) AS string_text,
    UNHEX(NULL) AS string_varbinary,
    UPPER('Hej') AS string_string
-- rules: string_bigint: mysql.string.bigint / mysql.string.bigint
-- rules: string_argument_type: mysql.string.argument-type / mysql.string.argument-type
-- rules: string_bigint_2: mysql.string.bigint-2 / mysql.string.bigint-2
-- rules: string_text: mysql.string.text / mysql.string.text
-- rules: string_varbinary: mysql.string.varbinary / mysql.string.varbinary
-- rules: string_string: mysql.string.string / mysql.string.string
```

```sql
SELECT
    LAST_INSERT_ID() AS system_bigint,
    COERCIBILITY(1000) AS system_int,
    DATABASE() AS system_text
-- rules: system_bigint: mysql.system.bigint / mysql.system.bigint
-- rules: system_int: mysql.system.int / mysql.system.int
-- rules: system_text: mysql.system.text-nullable / mysql.system.text-nullable
```

```sql
SELECT first_value(1) OVER () AS window_argument, percent_rank() OVER () AS window_double
-- rules: window_argument: common.window.value / common.window.value
-- rules: window_double: common.window.distribution / common.window.distribution
```

```sql
SELECT MOD(7, 3) AS math_argument_type
-- rules: math_argument_type: mysql.math.argument-type / mysql.math.argument-type
```

```sql
SELECT
    DATE('2003-12-31 01:02:03') AS datetime_date_nullable,
    sysdate() AS datetime_datetime,
    TIMESTAMP('2003-12-31') AS datetime_datetime_nullable,
    current_time AS datetime_time_non_null,
    TIME('2003-12-31 01:02:03') AS datetime_time_nullable,
    GET_FORMAT(DATE, 'USA') AS datetime_text_nullable
-- rules: datetime_date_nullable: mysql.datetime.date-nullable / mysql.datetime.date-nullable
-- rules: datetime_datetime: mysql.datetime.datetime / mysql.datetime.datetime
-- rules: datetime_datetime_nullable: mysql.datetime.datetime-nullable / mysql.datetime.datetime-nullable
-- rules: datetime_time_non_null: mysql.datetime.time-non-null / mysql.datetime.time-non-null
-- rules: datetime_time_nullable: mysql.datetime.time-nullable / mysql.datetime.time-nullable
-- rules: datetime_text_nullable: mysql.datetime.text-nullable / mysql.datetime.text-nullable
```

```sql
SELECT aes_decrypt('a', 'b') AS encryption_varbinary_nullable
-- rules: encryption_varbinary_nullable: mysql.encryption.varbinary-nullable / mysql.encryption.varbinary-nullable
```

```sql
SELECT JSON_SET('{"a": 1}', '$.a', 2) AS json_json
-- rules: json_json: mysql.json.json / mysql.json.json
```

```sql
SELECT JSON_EXTRACT('[10, 20, [30, 40]]', '$[1]') AS json_json_nullable
-- rules: json_json_nullable: mysql.json.json-nullable / mysql.json.json-nullable
```

```sql
SELECT JSON_VALUE('{"fname": "Joe", "lname": "Palmer"}', '$.fname') AS json_text_nullable
-- rules: json_text_nullable: mysql.json.text-nullable / mysql.json.text-nullable
```

```sql
SELECT EXP(0) AS math_double, rand() AS math_double_non_null
-- rules: math_double: mysql.math.double / mysql.math.double
-- rules: math_double_non_null: mysql.math.double-non-null / mysql.math.double-non-null
```

```sql
SELECT INET_ATON('10.0.5.9') AS misc_bigint_nullable
-- rules: misc_bigint_nullable: mysql.misc.bigint-nullable / mysql.misc.bigint-nullable
```

```sql
SELECT bin_to_uuid(1) AS misc_text
-- rules: misc_text: mysql.misc.text / mysql.misc.text
```

```sql
SELECT UUID() AS misc_text_non_null
-- rules: misc_text_non_null: mysql.misc.text-non-null / mysql.misc.text-non-null
```

```sql
SELECT inet6_aton(1) AS misc_varbinary_nullable
-- rules: misc_varbinary_nullable: mysql.misc.varbinary-nullable / mysql.misc.varbinary-nullable
```

```sql
SELECT GROUPING(i) AS misc_int_non_null FROM t GROUP BY i WITH ROLLUP
-- rules: misc_int_non_null: mysql.misc.int-non-null / mysql.misc.int-non-null
```

```sql
SELECT
    FIELD('Gg', 'Aa', 'Bb', 'Cc', 'Dd', 'Ff') AS string_bigint_2_non_null,
    QUOTE(NULL) AS string_text_non_null,
    REGEXP_SUBSTR('abc def ghi', '[a-z]+') AS string_text_nullable,
    from_base64(1) AS string_varbinary_nullable,
    CONCAT_WS(',', x'61') AS string_binary_any_argument_leading,
    ELT(1, x'61') AS string_binary_any_argument_nullable
-- rules: string_bigint_2_non_null: mysql.string.bigint-2-non-null / mysql.string.bigint-2-non-null
-- rules: string_text_non_null: mysql.string.text-non-null / mysql.string.text-non-null
-- rules: string_text_nullable: mysql.string.text-nullable / mysql.string.text-nullable
-- rules: string_varbinary_nullable: mysql.string.varbinary-nullable / mysql.string.varbinary-nullable
-- rules: string_binary_any_argument_leading: mysql.string.binary-any-argument-leading / mysql.string.binary-any-argument-leading
-- rules: string_binary_any_argument_nullable: mysql.string.binary-any-argument-nullable / mysql.string.binary-any-argument-nullable
```

```sql
SELECT REGEXP_SUBSTR(vb, tx) AS string_binary_nullable FROM t
-- rules: string_binary_nullable: mysql.string.binary-nullable / mysql.string.binary-nullable
```

```sql
SELECT REGEXP_REPLACE(tx, vb, tx) AS string_binary_pattern FROM t
-- rules: string_binary_pattern: mysql.string.binary-pattern / mysql.string.binary-pattern
```

```sql
SELECT USER() AS system_text
-- rules: system_text: mysql.system.text / mysql.system.text
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
    json_extract(tx, '$.a') AS extract, tx GLOB 'a*' AS glob, ~i AS "not"
FROM t
-- rules: bitwise: sqlite.bitwise / sqlite.bitwise
-- rules: division: common.arithmetic.division / common.arithmetic.division
-- rules: arrow: sqlite.json.arrow / sqlite.json.arrow
-- rules: long_arrow: sqlite.json.long-arrow / sqlite.json.long-arrow
-- rules: extract: sqlite.json.extract / sqlite.json.extract
-- rules: glob: common.pattern / common.pattern
-- rules: not: sqlite.bitwise.not / sqlite.bitwise.not
```

### Built-in functions

Each built-in function a rule types, one call per rule.

```sql
SELECT total(1) AS aggregate_double
-- rules: aggregate_double: sqlite.aggregate.double / sqlite.aggregate.double
```

```sql
SELECT unixepoch() AS datetime_bigint, julianday() AS datetime_double, date() AS datetime_text
-- rules: datetime_bigint: sqlite.datetime.bigint / sqlite.datetime.bigint
-- rules: datetime_double: sqlite.datetime.double / sqlite.datetime.double
-- rules: datetime_text: sqlite.datetime.text / sqlite.datetime.text
```

```sql
SELECT
    json_array_length(1) AS json_bigint,
    jsonb(1) AS json_blob,
    json(1) AS json_text,
    json_group_array(1) AS json_aggregate_text
-- rules: json_bigint: sqlite.json.bigint / sqlite.json.bigint
-- rules: json_blob: sqlite.json.blob / sqlite.json.blob
-- rules: json_text: sqlite.json.text / sqlite.json.text
-- rules: json_aggregate_text: sqlite.json.aggregate-text / sqlite.json.aggregate-text
```

```sql
SELECT ceiling(1) AS math_argument_type, sqrt(1) AS math_double
-- rules: math_argument_type: sqlite.math.argument-type / sqlite.math.argument-type
-- rules: math_double: sqlite.math.double-nullable / sqlite.math.double-nullable
```

```sql
SELECT
    abs(1) AS scalar_argument_type,
    iif('a', 'b') AS scalar_branches,
    ifnull('a', 'b') AS scalar_common,
    nullif('a', 'b') AS scalar_argument,
    glob('a', 'b') AS scalar_bigint,
    zeroblob(1) AS scalar_blob,
    round(1) AS scalar_double,
    ltrim(1) AS scalar_text
-- rules: scalar_argument_type: common.math.argument-type / common.math.argument-type
-- rules: scalar_branches: sqlite.scalar.branches / sqlite.scalar.branches
-- rules: scalar_common: sqlite.scalar.common / sqlite.scalar.common
-- rules: scalar_argument: common.conditional.nullif / common.conditional.nullif
-- rules: scalar_bigint: sqlite.scalar.bigint / sqlite.scalar.bigint
-- rules: scalar_blob: sqlite.scalar.blob / sqlite.scalar.blob
-- rules: scalar_double: sqlite.scalar.double / sqlite.scalar.double
-- rules: scalar_text: sqlite.scalar.text / sqlite.scalar.text
```

```sql
SELECT first_value(1) OVER () AS window_argument, percent_rank() OVER () AS window_double
-- rules: window_argument: common.window.value / common.window.value
-- rules: window_double: common.window.distribution / common.window.distribution
```

```sql
SELECT likely(1) AS scalar_argument, concat_ws(',', 'a', 'b') AS string_formatted
-- rules: scalar_argument: sqlite.scalar.argument / sqlite.scalar.argument
-- rules: string_formatted: common.string.formatted / common.string.formatted
```

```sql
SELECT json_object() AS json_text_non_null
-- rules: json_text_non_null: sqlite.json.text-non-null / sqlite.json.text-non-null
```

```sql
SELECT exp(1) AS math_double
-- rules: math_double: sqlite.math.double / sqlite.math.double
```

```sql
SELECT
    unhex(1) AS scalar_blob_nullable,
    printf() AS scalar_text_nullable,
    typeof(1) AS scalar_text_non_null
-- rules: scalar_blob_nullable: sqlite.scalar.blob-nullable / sqlite.scalar.blob-nullable
-- rules: scalar_text_nullable: sqlite.scalar.text-nullable / sqlite.scalar.text-nullable
-- rules: scalar_text_non_null: sqlite.scalar.text-non-null / sqlite.scalar.text-non-null
```

```sql
SELECT format() AS string_format
-- rules: string_format: sqlite.string.format / sqlite.string.format
```

## Other dialects

The rules of `common/` that every dialect above overrides apply in the others, except
those of `rules/dialects.toml`, such as `generic`, which read PostgreSQL's.

`schema.sql`:

```sql
CREATE TABLE t (j JSON NOT NULL, jb JSONB NOT NULL);
```

```toml
[tool.sqlty]
dialect = "ansi"
schema = "schema.sql"
```

```sql
SELECT j -> 'a' AS arrow, jb || jb AS jsonb, '{}' || jb AS jsonb_right FROM t
-- rules: arrow: common.json.arrow / common.json.arrow
-- rules: jsonb: common.concat.jsonb / common.concat.jsonb
-- rules: jsonb_right: common.concat.jsonb-right-operand / common.concat.jsonb-right-operand
```

## Generic SQL

`rules/dialects.toml` makes the `generic` dialect read PostgreSQL's rules, then MySQL's and
SQLite's for what PostgreSQL does not have, then `common/`.

```toml
[tool.sqlty]
dialect = "generic"
```

```sql
SELECT GREATEST(1, 2) AS greatest, CBRT(8) AS cbrt, RAND() AS rand, MONTHNAME(CURRENT_DATE) AS month, IIF(TRUE, 1, 2) AS iif, ~5 AS "not"
-- rules: greatest: postgres.conditional.common / postgres.conditional.common
-- rules: cbrt: postgres.math.double / postgres.math.double
-- rules: rand: mysql.math.double-non-null / mysql.math.double-non-null
-- rules: month: mysql.datetime.text / mysql.datetime.text
-- rules: iif: sqlite.scalar.branches / sqlite.scalar.branches
-- rules: not: postgres.bitwise.not / postgres.bitwise.not
-- columns: greatest: typing.Literal[1, 2] (None)
-- columns: cbrt: float (DOUBLE)
-- columns: rand: float (DOUBLE)
-- columns: month: str (TEXT)
-- columns: iif: typing.Literal[1, 2] (None)
-- columns: not: int (None)
```
