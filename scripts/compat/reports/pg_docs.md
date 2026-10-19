# PostgreSQL 17 documentation examples

706 examples of the function and operator tables, `scripts/compat/pg_docs.py`: 543 columns agree with the server.

## Typed differently

None.

## Untyped

| Page | Example | Server type |
|---|---|---|
| functions-binarystring.html | `SELECT convert('text_in_utf8', 'UTF8', 'LATIN1')` | `bytea` |
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
| functions-geometry.html | `SELECT area(box '(2,2),(0,0)')` | `float8` |
| functions-geometry.html | `SELECT center(box '(1,2),(0,0)')` | `point` |
| functions-geometry.html | `SELECT diagonal(box '(1,2),(0,0)')` | `lseg` |
| functions-geometry.html | `SELECT diameter(circle '<(0,0),2>')` | `float8` |
| functions-geometry.html | `SELECT height(box '(1,2),(0,0)')` | `float8` |
| functions-geometry.html | `SELECT isclosed(path '((0,0),(1,1),(2,0))')` | `bool` |
| functions-geometry.html | `SELECT isopen(path '[(0,0),(1,1),(2,0)]')` | `bool` |
| functions-geometry.html | `SELECT npoints(path '[(0,0),(1,1),(2,0)]')` | `int4` |
| functions-geometry.html | `SELECT pclose(path '[(0,0),(1,1),(2,0)]')` | `path` |
| functions-geometry.html | `SELECT popen(path '((0,0),(1,1),(2,0))')` | `path` |
| functions-geometry.html | `SELECT radius(circle '<(0,0),2>')` | `float8` |
| functions-geometry.html | `SELECT slope(point '(0,0)', point '(2,1)')` | `float8` |
| functions-geometry.html | `SELECT width(box '(1,2),(0,0)')` | `float8` |
| functions-geometry.html | `SELECT box(circle '<(0,0),2>')` | `box` |
| functions-geometry.html | `SELECT box(point '(1,0)')` | `box` |
| functions-geometry.html | `SELECT box(point '(0,1)', point '(1,0)')` | `box` |
| functions-geometry.html | `SELECT box(polygon '((0,0),(1,1),(2,0))')` | `box` |
| functions-geometry.html | `SELECT bound_box(box '(1,1),(0,0)', box '(4,4),(3,3)')` | `box` |
| functions-geometry.html | `SELECT circle(box '(1,1),(0,0)')` | `circle` |
| functions-geometry.html | `SELECT circle(point '(0,0)', 2.0)` | `circle` |
| functions-geometry.html | `SELECT circle(polygon '((0,0),(1,3),(2,0))')` | `circle` |
| functions-geometry.html | `SELECT line(point '(-1,0)', point '(1,0)')` | `line` |
| functions-geometry.html | `SELECT lseg(box '(1,0),(-1,0)')` | `lseg` |
| functions-geometry.html | `SELECT lseg(point '(-1,0)', point '(1,0)')` | `lseg` |
| functions-geometry.html | `SELECT path(polygon '((0,0),(1,1),(2,0))')` | `path` |
| functions-geometry.html | `SELECT point(23.4, -44.5)` | `point` |
| functions-geometry.html | `SELECT point(box '(1,0),(-1,0)')` | `point` |
| functions-geometry.html | `SELECT point(circle '<(0,0),2>')` | `point` |
| functions-geometry.html | `SELECT point(lseg '[(-1,0),(1,0)]')` | `point` |
| functions-geometry.html | `SELECT point(polygon '((0,0),(1,1),(2,0))')` | `point` |
| functions-geometry.html | `SELECT polygon(box '(1,1),(0,0)')` | `polygon` |
| functions-geometry.html | `SELECT polygon(circle '<(0,0),2>')` | `polygon` |
| functions-geometry.html | `SELECT polygon(4, circle '<(3,0),1>')` | `polygon` |
| functions-geometry.html | `SELECT polygon(path '((0,0),(1,1),(2,0))')` | `polygon` |
| functions-info.html | `SELECT pg_input_is_valid('42', 'integer')` | `bool` |
| functions-info.html | `SELECT pg_input_is_valid('42000000000', 'integer')` | `bool` |
| functions-info.html | `SELECT pg_input_is_valid('1234.567', 'numeric(7,4)')` | `bool` |
| functions-json.html | `SELECT array_to_json('{{1,5},{99,100}}'::int[])` | `json` |
| functions-json.html | `SELECT jsonb_set_lax('[{"f1":1,"f2":null},2,null,3]', '{0,f1}', null)` | `jsonb` |
| functions-json.html | `SELECT jsonb_set_lax('[{"f1":99,"f2":null},2]', '{0,f3}', null, true, 'return_target')` | `jsonb` |
| functions-json.html | `select * from jsonb_path_query('{"a":[1,2,3,4,5]}', '$.a[*] ? (@ >= $min && @ <= $max)', '{"min":2, "max":4}')` | `jsonb` |
| functions-math.html | `SELECT div(9, 4)` | `numeric` |
| functions-math.html | `SELECT gcd(1071, 462)` | `int4` |
| functions-math.html | `SELECT lcm(1071, 462)` | `int4` |
| functions-net.html | `SELECT inet_same_family(inet '192.168.1.5/24', inet '::1')` | `bool` |
| functions-net.html | `SELECT trunc(macaddr '12:34:56:78:90:ab')` | `macaddr` |
| functions-net.html | `SELECT trunc(macaddr8 '12:34:56:78:90:ab:cd:ef')` | `macaddr8` |
| functions-net.html | `SELECT macaddr8_set7bit(macaddr8 '00:34:56:ab:cd:ef')` | `macaddr8` |
| functions-range.html | `SELECT int8range(1,10) << int8range(100,110)` | `bool` |
| functions-range.html | `SELECT int8range(50,60) >> int8range(20,30)` | `bool` |
| functions-range.html | `SELECT numrange(5,15) + numrange(10,20)` | `numrange` |
| functions-range.html | `SELECT int8range(5,15) * int8range(10,20)` | `int8range` |
| functions-range.html | `SELECT int8range(5,15) - int8range(10,20)` | `int8range` |
| functions-range.html | `SELECT '{[1,10)}'::int8multirange << '{[100,110)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[1,10)}'::int8multirange << int8range(100,110)` | `bool` |
| functions-range.html | `SELECT int8range(1,10) << '{[100,110)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[50,60)}'::int8multirange >> '{[20,30)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[50,60)}'::int8multirange >> int8range(20,30)` | `bool` |
| functions-range.html | `SELECT int8range(50,60) >> '{[20,30)}'::int8multirange` | `bool` |
| functions-range.html | `SELECT '{[5,10)}'::nummultirange + '{[15,20)}'::nummultirange` | `nummultirange` |
| functions-range.html | `SELECT '{[5,15)}'::int8multirange * '{[10,20)}'::int8multirange` | `int8multirange` |
| functions-range.html | `SELECT '{[5,20)}'::int8multirange - '{[10,15)}'::int8multirange` | `int8multirange` |
| functions-range.html | `SELECT range_merge('[1,2)'::int4range, '[3,4)'::int4range)` | `int4range` |
| functions-range.html | `SELECT lower('{[1.1,2.2)}'::nummultirange)` | `numeric` |
| functions-range.html | `SELECT upper('{[1.1,2.2)}'::nummultirange)` | `numeric` |
| functions-range.html | `SELECT range_merge('{[1,2), [3,4)}'::int4multirange)` | `int4range` |
| functions-range.html | `SELECT multirange('[1,2)'::int4range)` | `int4multirange` |
| functions-range.html | `SELECT unnest('{[1,2), [3,4)}'::int4multirange)` | `int4range` |
| functions-string.html | `SELECT pg_client_encoding()` | `name` |
| functions-string.html | `SELECT to_ascii('Karél')` | `text` |
| functions-textsearch.html | `SELECT !! 'cat'::tsquery` | `tsquery` |
| functions-textsearch.html | `SELECT array_to_tsvector('{fat,cat,rat}'::text[])` | `tsvector` |
| functions-textsearch.html | `SELECT get_current_ts_config()` | `regconfig` |
| functions-textsearch.html | `SELECT numnode('(fat & rat) \| cat'::tsquery)` | `int4` |
| functions-textsearch.html | `SELECT querytree('foo & ! bar'::tsquery)` | `text` |
| functions-textsearch.html | `SELECT strip('fat:2,4 cat:3 rat:5A'::tsvector)` | `tsvector` |
| functions-textsearch.html | `SELECT json_to_tsvector('english', '{"a": "The Fat Rats", "b": 123}'::json, '["string", "numeric"]')` | `tsvector` |
| functions-textsearch.html | `SELECT json_to_tsvector('english', '{"cat": "The Fat Rats", "dog": 123}'::json, '"all"')` | `tsvector` |
| functions-textsearch.html | `SELECT ts_delete('fat:2,4 cat:3 rat:5A'::tsvector, 'fat')` | `tsvector` |
| functions-textsearch.html | `SELECT ts_delete('fat:2,4 cat:3 rat:5A'::tsvector, ARRAY['fat','rat'])` | `tsvector` |
| functions-textsearch.html | `SELECT ts_filter('fat:2,4 cat:3b,7c rat:5A'::tsvector, '{a,b}')` | `tsvector` |
| functions-textsearch.html | `SELECT ts_rewrite('a & b'::tsquery, 'a'::tsquery, 'foo\|bar'::tsquery)` | `tsquery` |
| functions-textsearch.html | `SELECT ts_rewrite('a & b'::tsquery, 'SELECT t,s FROM aliases')` | `tsquery` |
| functions-textsearch.html | `SELECT tsquery_phrase(to_tsquery('fat'), to_tsquery('cat'))` | `tsquery` |
| functions-textsearch.html | `SELECT tsquery_phrase(to_tsquery('fat'), to_tsquery('cat'), 10)` | `tsquery` |
| functions-textsearch.html | `SELECT tsvector_to_array('fat:2,4 cat:3 rat:5A'::tsvector)` | `text[]` |
| functions-textsearch.html | `SELECT ts_debug('english', 'The Brightest supernovaes')` | `record` |
| functions-textsearch.html | `SELECT ts_lexize('english_stem', 'stars')` | `text[]` |
| functions-textsearch.html | `SELECT ts_parse('default', 'foo - bar')` | `record` |
| functions-textsearch.html | `SELECT ts_parse(3722, 'foo - bar')` | `record` |
| functions-textsearch.html | `SELECT ts_token_type('default')` | `record` |
| functions-textsearch.html | `SELECT ts_token_type(3722)` | `record` |
| functions-textsearch.html | `SELECT ts_stat('SELECT vector FROM apod')` | `record` |

## Not parsed

| Page | Example |
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

## Rejected by the server

The example needs a table, type or role the documentation creates elsewhere.

| Page | Example | Error |
|---|---|---|
| functions-enum.html | `SELECT enum_first(null::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_last(null::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range(null::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range('orange'::rainbow, 'green'::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range(NULL, 'green'::rainbow)` | ERROR:  type "rainbow" does not exist |
| functions-enum.html | `SELECT enum_range('orange'::rainbow, NULL)` | ERROR:  type "rainbow" does not exist |
| functions-info.html | `SELECT 'calvin=r*w/hobbes'::aclitem = 'calvin=r*w*/hobbes'::aclitem` | ERROR:  role "calvin" does not exist |
| functions-info.html | `SELECT '{calvin=r*w/hobbes,hobbes=r*w*/postgres}'::aclitem[] @> 'calvin=r*/hobbes'::aclitem` | ERROR:  role "calvin" does not exist |
| functions-info.html | `SELECT pg_basetype('mytext'::regtype)` | ERROR:  type "mytext" does not exist |
| functions-json.html | `select * from json_populate_record(null::myrowtype, '{"a": 1, "b": ["2", "a b"], "c": {"d": 4, "e": "a b c"}, "x": "foo"}')` | ERROR:  type "myrowtype" does not exist |
| functions-json.html | `select jsonb_populate_record_valid(NULL::jsb_char2, '{"a": "aaa"}')` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select * from jsonb_populate_record(NULL::jsb_char2, '{"a": "aaa"}') q` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select jsonb_populate_record_valid(NULL::jsb_char2, '{"a": "aa"}')` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select * from jsonb_populate_record(NULL::jsb_char2, '{"a": "aa"}') q` | ERROR:  type "jsb_char2" does not exist |
| functions-json.html | `select * from json_populate_recordset(null::twoints, '[{"a":1,"b":2}, {"a":3,"b":4}]')` | ERROR:  type "twoints" does not exist |
| functions-json.html | `select * from json_to_record('{"a":1,"b":[1,2,3],"c":[1,2,3],"e":"bar","r": {"a": 123, "b": "a b c"}}') as x(a int, b text, c int[], d text, r myrowtype)` | ERROR:  type "myrowtype" does not exist |
