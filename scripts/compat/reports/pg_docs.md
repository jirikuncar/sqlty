# PostgreSQL 17 functions and operators

2553 queries, `scripts/compat/pg_docs.py`: the examples of the documentation's function and operator tables, each `pg_proc` overload of a documented function, and each `pg_operator` operator. 2282 columns agree with the server.

## Typed differently

None.

## Untyped

| Source | Query | Server type |
|---|---|---|
| functions-array.html | `SELECT ARRAY[1,2,3] \|\| ARRAY[[4,5,6],[7,8,9.9]]` | `numeric[]` |
| functions-bitstring.html | `SELECT B'10001' \|\| B'011'` | `varbit` |
| functions-bitstring.html | `SELECT B'10001' & B'01101'` | `bit` |
| functions-bitstring.html | `SELECT B'10001' \| B'01101'` | `bit` |
| functions-bitstring.html | `SELECT B'10001' # B'01101'` | `bit` |
| functions-bitstring.html | `SELECT B'10001' << 3` | `bit` |
| functions-bitstring.html | `SELECT B'10001' >> 2` | `bit` |
| functions-datetime.html | `SELECT time with time zone '05:34:17-05' at time zone 'UTC'` | `timetz` |
| functions-geometry.html | `SELECT box '(1,1),(0,0)' + point '(2,0)'` | `box` |
| functions-geometry.html | `SELECT path '[(0,0),(1,1)]' + path '[(2,2),(3,3),(4,4)]'` | `path` |
| functions-geometry.html | `SELECT box '(1,1),(0,0)' - point '(2,0)'` | `box` |
| functions-geometry.html | `SELECT path '((0,0),(1,0),(1,1))' * point '(3.0,0)'` | `path` |
| functions-geometry.html | `SELECT path '((0,0),(1,0),(1,1))' * point(cosd(45), sind(45))` | `path` |
| functions-geometry.html | `SELECT path '((0,0),(1,0),(1,1))' / point '(2.0,0)'` | `path` |
| functions-geometry.html | `SELECT path '((0,0),(1,0),(1,1))' / point(cosd(45), sind(45))` | `path` |
| functions-geometry.html | `SELECT @-@ path '[(0,0),(1,0),(1,1)]'` | `float8` |
| functions-geometry.html | `SELECT @@ box '(2,2),(0,0)'` | `point` |
| functions-geometry.html | `SELECT # path '((1,0),(0,1),(-1,0))'` | `int4` |
| functions-geometry.html | `SELECT lseg '[(0,0),(1,1)]' # lseg '[(1,0),(0,1)]'` | `point` |
| functions-geometry.html | `SELECT box '(2,2),(-1,-1)' # box '(1,1),(-2,-2)'` | `box` |
| functions-geometry.html | `SELECT point '(0,0)' ## lseg '[(2,0),(0,2)]'` | `point` |
| functions-geometry.html | `SELECT circle '<(0,0),1>' <-> circle '<(5,0),1>'` | `float8` |
| functions-geometry.html | `SELECT circle '<(0,0),1>' << circle '<(5,0),1>'` | `bool` |
| functions-geometry.html | `SELECT circle '<(5,0),1>' >> circle '<(0,0),1>'` | `bool` |
| functions-geometry.html | `SELECT box '(3,3),(0,0)' <<\| box '(5,5),(3,4)'` | `bool` |
| functions-geometry.html | `SELECT box '(5,5),(3,4)' \|>> box '(3,3),(0,0)'` | `bool` |
| functions-geometry.html | `SELECT box '(1,1),(0,0)' &<\| box '(2,2),(0,0)'` | `bool` |
| functions-geometry.html | `SELECT box '(3,3),(0,0)' \|&> box '(2,2),(0,0)'` | `bool` |
| functions-geometry.html | `SELECT box '((1,1),(0,0))' <^ box '((2,2),(1,1))'` | `bool` |
| functions-geometry.html | `SELECT box '((2,2),(1,1))' >^ box '((1,1),(0,0))'` | `bool` |
| functions-geometry.html | `SELECT lseg '[(-1,0),(1,0)]' ?# box '(2,2),(-2,-2)'` | `bool` |
| functions-geometry.html | `SELECT ?- lseg '[(-1,0),(1,0)]'` | `bool` |
| functions-geometry.html | `SELECT point '(1,0)' ?- point '(0,0)'` | `bool` |
| functions-geometry.html | `SELECT ?\| lseg '[(-1,0),(1,0)]'` | `bool` |
| functions-geometry.html | `SELECT lseg '[(0,0),(0,1)]' ?-\| lseg '[(0,0),(1,0)]'` | `bool` |
| functions-geometry.html | `SELECT lseg '[(-1,0),(1,0)]' ?\|\| lseg '[(-1,2),(1,2)]'` | `bool` |
| functions-geometry.html | `SELECT polygon '((0,0),(1,1))' ~= polygon '((1,1),(0,0))'` | `bool` |
| functions-json.html | `select * from jsonb_path_query('{"a":[1,2,3,4,5]}', '$.a[*] ? (@ >= $min && @ <= $max)', '{"min":2, "max":4}')` | `jsonb` |
| functions-range.html | `SELECT int8range(1,10) << int8range(100,110)` | `bool` |
| functions-range.html | `SELECT int8range(50,60) >> int8range(20,30)` | `bool` |
| functions-range.html | `SELECT '{[1,10)}'::int8multirange << '{[100,110)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[1,10)}'::int8multirange << int8range(100,110)` | `bool` |
| functions-range.html | `SELECT int8range(1,10) << '{[100,110)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[50,60)}'::int8multirange >> '{[20,30)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[50,60)}'::int8multirange >> int8range(20,30)` | `bool` |
| functions-range.html | `SELECT int8range(50,60) >> '{[20,30)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[5,10)}'::nummultirange + '{[15,20)}'::nummultirange` | `nummultirange` |
| functions-range.html | `SELECT '{[5,15)}'::int8multirange * '{[10,20)}'::int8multirange` | `int8multirange` |
| functions-range.html | `SELECT '{[5,20)}'::int8multirange - '{[10,15)}'::int8multirange` | `int8multirange` |
| functions-textsearch.html | `SELECT !! 'cat'::tsquery` | `tsquery` |
| pg_proc | `SELECT pg_event_trigger_ddl_commands()` | `record` |
| pg_proc | `SELECT pg_event_trigger_dropped_objects()` | `record` |
| pg_proc | `SELECT pg_mcv_list_items(NULL::pg_mcv_list)` | `record` |
| pg_proc | `SELECT suppress_redundant_updates_trigger()` | `trigger` |
| pg_proc | `SELECT tsvector_update_trigger()` | `trigger` |
| pg_proc | `SELECT tsvector_update_trigger_column()` | `trigger` |
| pg_operator | `SELECT !! NULL::tsquery` | `tsquery` |
| pg_operator | `SELECT # NULL::path` | `int4` |
| pg_operator | `SELECT # NULL::polygon` | `int4` |
| pg_operator | `SELECT NULL::bit # NULL::bit` | `bit` |
| pg_operator | `SELECT NULL::box # NULL::box` | `box` |
| pg_operator | `SELECT NULL::line # NULL::line` | `point` |
| pg_operator | `SELECT NULL::lseg # NULL::lseg` | `point` |
| pg_operator | `SELECT NULL::line ## NULL::lseg` | `point` |
| pg_operator | `SELECT NULL::lseg ## NULL::box` | `point` |
| pg_operator | `SELECT NULL::lseg ## NULL::lseg` | `point` |
| pg_operator | `SELECT NULL::point ## NULL::box` | `point` |
| pg_operator | `SELECT NULL::point ## NULL::line` | `point` |
| pg_operator | `SELECT NULL::point ## NULL::lseg` | `point` |
| pg_operator | `SELECT NULL::bit & NULL::bit` | `bit` |
| pg_operator | `SELECT NULL::macaddr & NULL::macaddr` | `macaddr` |
| pg_operator | `SELECT NULL::macaddr8 & NULL::macaddr8` | `macaddr8` |
| pg_operator | `SELECT NULL::box &<\| NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle &<\| NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::polygon &<\| NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::int4multirange * NULL::int4multirange` | `int4multirange` |
| pg_operator | `SELECT NULL::bigint * NULL::money` | `money` |
| pg_operator | `SELECT NULL::box * NULL::point` | `box` |
| pg_operator | `SELECT NULL::circle * NULL::point` | `circle` |
| pg_operator | `SELECT NULL::double precision * NULL::money` | `money` |
| pg_operator | `SELECT NULL::integer * NULL::money` | `money` |
| pg_operator | `SELECT NULL::money * NULL::bigint` | `money` |
| pg_operator | `SELECT NULL::money * NULL::double precision` | `money` |
| pg_operator | `SELECT NULL::money * NULL::integer` | `money` |
| pg_operator | `SELECT NULL::money * NULL::real` | `money` |
| pg_operator | `SELECT NULL::money * NULL::smallint` | `money` |
| pg_operator | `SELECT NULL::path * NULL::point` | `path` |
| pg_operator | `SELECT NULL::point * NULL::point` | `point` |
| pg_operator | `SELECT NULL::real * NULL::money` | `money` |
| pg_operator | `SELECT NULL::smallint * NULL::money` | `money` |
| pg_operator | `SELECT NULL::aclitem[] + NULL::aclitem` | `aclitem[]` |
| pg_operator | `SELECT NULL::int4multirange + NULL::int4multirange` | `int4multirange` |
| pg_operator | `SELECT NULL::box + NULL::point` | `box` |
| pg_operator | `SELECT NULL::circle + NULL::point` | `circle` |
| pg_operator | `SELECT NULL::money + NULL::money` | `money` |
| pg_operator | `SELECT NULL::numeric + NULL::pg_lsn` | `pg_lsn` |
| pg_operator | `SELECT NULL::path + NULL::path` | `path` |
| pg_operator | `SELECT NULL::path + NULL::point` | `path` |
| pg_operator | `SELECT NULL::pg_lsn + NULL::numeric` | `pg_lsn` |
| pg_operator | `SELECT NULL::point + NULL::point` | `point` |
| pg_operator | `SELECT NULL::aclitem[] - NULL::aclitem` | `aclitem[]` |
| pg_operator | `SELECT NULL::int4multirange - NULL::int4multirange` | `int4multirange` |
| pg_operator | `SELECT NULL::box - NULL::point` | `box` |
| pg_operator | `SELECT NULL::circle - NULL::point` | `circle` |
| pg_operator | `SELECT NULL::money - NULL::money` | `money` |
| pg_operator | `SELECT NULL::path - NULL::point` | `path` |
| pg_operator | `SELECT NULL::pg_lsn - NULL::numeric` | `pg_lsn` |
| pg_operator | `SELECT NULL::pg_lsn - NULL::pg_lsn` | `numeric` |
| pg_operator | `SELECT NULL::point - NULL::point` | `point` |
| pg_operator | `SELECT NULL::box / NULL::point` | `box` |
| pg_operator | `SELECT NULL::circle / NULL::point` | `circle` |
| pg_operator | `SELECT NULL::money / NULL::bigint` | `money` |
| pg_operator | `SELECT NULL::money / NULL::double precision` | `money` |
| pg_operator | `SELECT NULL::money / NULL::integer` | `money` |
| pg_operator | `SELECT NULL::money / NULL::money` | `float8` |
| pg_operator | `SELECT NULL::money / NULL::real` | `money` |
| pg_operator | `SELECT NULL::money / NULL::smallint` | `money` |
| pg_operator | `SELECT NULL::path / NULL::point` | `path` |
| pg_operator | `SELECT NULL::point / NULL::point` | `point` |
| pg_operator | `SELECT NULL::box <-> NULL::box` | `float8` |
| pg_operator | `SELECT NULL::box <-> NULL::lseg` | `float8` |
| pg_operator | `SELECT NULL::box <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::circle <-> NULL::circle` | `float8` |
| pg_operator | `SELECT NULL::circle <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::circle <-> NULL::polygon` | `float8` |
| pg_operator | `SELECT NULL::line <-> NULL::line` | `float8` |
| pg_operator | `SELECT NULL::line <-> NULL::lseg` | `float8` |
| pg_operator | `SELECT NULL::line <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::lseg <-> NULL::box` | `float8` |
| pg_operator | `SELECT NULL::lseg <-> NULL::line` | `float8` |
| pg_operator | `SELECT NULL::lseg <-> NULL::lseg` | `float8` |
| pg_operator | `SELECT NULL::lseg <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::path <-> NULL::path` | `float8` |
| pg_operator | `SELECT NULL::path <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::box` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::circle` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::line` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::lseg` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::path` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::point <-> NULL::polygon` | `float8` |
| pg_operator | `SELECT NULL::polygon <-> NULL::circle` | `float8` |
| pg_operator | `SELECT NULL::polygon <-> NULL::point` | `float8` |
| pg_operator | `SELECT NULL::polygon <-> NULL::polygon` | `float8` |
| pg_operator | `SELECT NULL::int4multirange << NULL::int4multirange` | `bool` |
| pg_operator | `SELECT NULL::int4multirange << NULL::int4range` | `bool` |
| pg_operator | `SELECT NULL::int4range << NULL::int4multirange` | `bool` |
| pg_operator | `SELECT NULL::int4range << NULL::int4range` | `bool` |
| pg_operator | `SELECT NULL::bit << NULL::integer` | `bit` |
| pg_operator | `SELECT NULL::box << NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle << NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::point << NULL::point` | `bool` |
| pg_operator | `SELECT NULL::polygon << NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::box <<\| NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle <<\| NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::point <<\| NULL::point` | `bool` |
| pg_operator | `SELECT NULL::polygon <<\| NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::box <^ NULL::box` | `bool` |
| pg_operator | `SELECT NULL::point <^ NULL::point` | `bool` |
| pg_operator | `SELECT NULL::int4multirange >> NULL::int4multirange` | `bool` |
| pg_operator | `SELECT NULL::int4multirange >> NULL::int4range` | `bool` |
| pg_operator | `SELECT NULL::int4range >> NULL::int4multirange` | `bool` |
| pg_operator | `SELECT NULL::int4range >> NULL::int4range` | `bool` |
| pg_operator | `SELECT NULL::bit >> NULL::integer` | `bit` |
| pg_operator | `SELECT NULL::box >> NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle >> NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::point >> NULL::point` | `bool` |
| pg_operator | `SELECT NULL::polygon >> NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::box >^ NULL::box` | `bool` |
| pg_operator | `SELECT NULL::point >^ NULL::point` | `bool` |
| pg_operator | `SELECT NULL::box ?# NULL::box` | `bool` |
| pg_operator | `SELECT NULL::line ?# NULL::box` | `bool` |
| pg_operator | `SELECT NULL::line ?# NULL::line` | `bool` |
| pg_operator | `SELECT NULL::lseg ?# NULL::box` | `bool` |
| pg_operator | `SELECT NULL::lseg ?# NULL::line` | `bool` |
| pg_operator | `SELECT NULL::lseg ?# NULL::lseg` | `bool` |
| pg_operator | `SELECT NULL::path ?# NULL::path` | `bool` |
| pg_operator | `SELECT ?- NULL::line` | `bool` |
| pg_operator | `SELECT ?- NULL::lseg` | `bool` |
| pg_operator | `SELECT NULL::point ?- NULL::point` | `bool` |
| pg_operator | `SELECT NULL::line ?-\| NULL::line` | `bool` |
| pg_operator | `SELECT NULL::lseg ?-\| NULL::lseg` | `bool` |
| pg_operator | `SELECT ?\| NULL::line` | `bool` |
| pg_operator | `SELECT ?\| NULL::lseg` | `bool` |
| pg_operator | `SELECT NULL::line ?\|\| NULL::line` | `bool` |
| pg_operator | `SELECT NULL::lseg ?\|\| NULL::lseg` | `bool` |
| pg_operator | `SELECT @-@ NULL::lseg` | `float8` |
| pg_operator | `SELECT @-@ NULL::path` | `float8` |
| pg_operator | `SELECT @@ NULL::box` | `point` |
| pg_operator | `SELECT @@ NULL::circle` | `point` |
| pg_operator | `SELECT @@ NULL::lseg` | `point` |
| pg_operator | `SELECT @@ NULL::polygon` | `point` |
| pg_operator | `SELECT NULL::bit \| NULL::bit` | `bit` |
| pg_operator | `SELECT NULL::macaddr \| NULL::macaddr` | `macaddr` |
| pg_operator | `SELECT NULL::macaddr8 \| NULL::macaddr8` | `macaddr8` |
| pg_operator | `SELECT NULL::box \|&> NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle \|&> NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::polygon \|&> NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::box \|>> NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle \|>> NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::point \|>> NULL::point` | `bool` |
| pg_operator | `SELECT NULL::polygon \|>> NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::bit varying \|\| NULL::bit varying` | `varbit` |
| pg_operator | `SELECT NULL::character ~<=~ NULL::character` | `bool` |
| pg_operator | `SELECT NULL::text ~<=~ NULL::text` | `bool` |
| pg_operator | `SELECT NULL::character ~<~ NULL::character` | `bool` |
| pg_operator | `SELECT NULL::text ~<~ NULL::text` | `bool` |
| pg_operator | `SELECT NULL::box ~= NULL::box` | `bool` |
| pg_operator | `SELECT NULL::circle ~= NULL::circle` | `bool` |
| pg_operator | `SELECT NULL::point ~= NULL::point` | `bool` |
| pg_operator | `SELECT NULL::polygon ~= NULL::polygon` | `bool` |
| pg_operator | `SELECT NULL::character ~>=~ NULL::character` | `bool` |
| pg_operator | `SELECT NULL::text ~>=~ NULL::text` | `bool` |
| pg_operator | `SELECT NULL::character ~>~ NULL::character` | `bool` |
| pg_operator | `SELECT NULL::text ~>~ NULL::text` | `bool` |

## Not parsed

| Source | Query |
|---|---|
| functions-binarystring.html | `SELECT trim(both from '\x1234567890'::bytea, '\x9012'::bytea)` |
| functions-comparison.html | `SELECT 2 BETWEEN SYMMETRIC 3 AND 1` |
| functions-comparison.html | `SELECT 2 NOT BETWEEN SYMMETRIC 3 AND 1` |
| functions-datetime.html | `SELECT timestamp '2001-02-16 20:38:40' at local` |
| functions-datetime.html | `SELECT timestamp with time zone '2001-02-16 20:38:40-05' at local` |
| functions-datetime.html | `SELECT time with time zone '05:34:17-05' at local` |
| functions-info.html | `SELECT collation for ('foo'::text)` |
| functions-info.html | `SELECT collation for ('foo' COLLATE "de_DE")` |
| functions-info.html | `SELECT * FROM pg_input_error_info('42000000000', 'integer')` |
| functions-json.html | `SELECT json_array(SELECT * FROM (VALUES(1),(2)) t)` |
| functions-json.html | `SELECT JSON_EXISTS(jsonb '{"key1": [1,2,3]}', 'strict $.key1[*] ? (@ > $x)' PASSING 2 AS x)` |
| functions-json.html | `SELECT JSON_EXISTS(jsonb '{"a": [1,2,3]}', 'lax $.a[5]' ERROR ON ERROR)` |
| functions-json.html | `SELECT JSON_EXISTS(jsonb '{"a": [1,2,3]}', 'strict $.a[5]' ERROR ON ERROR)` |
| functions-json.html | `SELECT JSON_QUERY(jsonb '[1,[2,3],null]', 'lax $[*][$off]' PASSING 1 AS off WITH CONDITIONAL WRAPPER)` |
| functions-json.html | `SELECT JSON_QUERY(jsonb '{"a": "[1, 2]"}', 'lax $.a' OMIT QUOTES)` |
| functions-json.html | `SELECT JSON_QUERY(jsonb '{"a": "[1, 2]"}', 'lax $.a' RETURNING int[] OMIT QUOTES ERROR ON ERROR)` |
| functions-json.html | `SELECT JSON_VALUE(jsonb '[1,2]', 'strict $[$off]' PASSING 1 as off)` |
| functions-json.html | `SELECT JSON_VALUE(jsonb '[1,2]', 'strict $[*]' DEFAULT 9 ON ERROR)` |
| functions-string.html | `SELECT substring('Thomas' similar '%#"o_a#"_' escape '#')` |
| functions-string.html | `SELECT trim(both from 'yxTomxx', 'xyz')` |
| functions-textsearch.html | `select * from unnest('cat:3 fat:2,4 rat:5A'::tsvector)` |

## Examples rejected by the server

The example needs a table, type or role the documentation creates elsewhere.

| Source | Query | Error |
|---|---|---|
| functions-enum.html | `SELECT enum_first(null::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_last(null::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range(null::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range('orange'::rainbow, 'green'::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range(NULL, 'green'::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range('orange'::rainbow, NULL)` | ERROR:  type "rainbow" does not exist |
| functions-info.html | `SELECT 'calvin=r*w/hobbes'::aclitem = 'calvin=r*w*/hobbes'::aclitem` | ERROR:  role "calvin" does not exist |
| functions-info.html | `SELECT '{calvin=r*w/hobbes,hobbes=r*w*/postgres}'::aclitem[] @> 'calvin=r*/hobbes'::aclitem` | ERROR:  role "calvin" does not exist |
| functions-info.html | `SELECT '{calvin=r*w/hobbes,hobbes=r*w*/postgres}'::aclitem[] ~ 'calvin=r*/hobbes'::aclitem` | ERROR:  role "calvin" does not exist |
| functions-info.html | `SELECT pg_basetype('mytext'::regtype)` | ERROR:  type "mytext" does not exist |
| functions-json.html | `select * from json_populate_record(null::myrowtype, '{"a": 1, "b": ["2", "a b"], "c": {"d": 4, "e": "a b c"}, "x": "foo"}')` | ERROR:  type "myrowtype" does not exist |
| functions-json.html | `select jsonb_populate_record_valid(NULL::jsb_char2, '{"a": "aaa"}')` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select * from jsonb_populate_record(NULL::jsb_char2, '{"a": "aaa"}') q` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select jsonb_populate_record_valid(NULL::jsb_char2, '{"a": "aa"}')` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select * from jsonb_populate_record(NULL::jsb_char2, '{"a": "aa"}') q` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select * from json_populate_recordset(null::twoints, '[{"a":1,"b":2}, {"a":3,"b":4}]')` | ERROR:  type "twoints" does not exist |
| functions-json.html | `select * from json_to_record('{"a":1,"b":[1,2,3],"c":[1,2,3],"e":"bar","r": {"a": 123, "b": "a b c"}}') as x(a int, b text, c int[], d text, r myrowtype)` | ERROR:  type "myrowtype" does not exist |
