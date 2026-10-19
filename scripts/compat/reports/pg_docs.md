# PostgreSQL 17 functions and operators

2553 queries, `scripts/compat/pg_docs.py`: the examples of the documentation's function and operator tables, each `pg_proc` overload of a documented function, and each `pg_operator` operator. 1814 columns agree with the server.

## Typed differently

None.

## Untyped

| Source | Query | Server type |
|---|---|---|
| functions-array.html | `SELECT ARRAY[1,2,3] \|\| ARRAY[[4,5,6],[7,8,9.9]]` | `numeric[]` |
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
| pg_proc | `SELECT acldefault(NULL::"char", NULL::oid)` | `aclitem[]` |
| pg_proc | `SELECT aclexplode(NULL::aclitem[])` | `record` |
| pg_proc | `SELECT area(NULL::box)` | `float8` |
| pg_proc | `SELECT area(NULL::circle)` | `float8` |
| pg_proc | `SELECT area(NULL::path)` | `float8` |
| pg_proc | `SELECT array_to_json(NULL::int4[])` | `json` |
| pg_proc | `SELECT array_to_json(NULL::int4[], NULL::boolean)` | `json` |
| pg_proc | `SELECT array_to_tsvector(NULL::text[])` | `tsvector` |
| pg_proc | `SELECT bound_box(NULL::box, NULL::box)` | `box` |
| pg_proc | `SELECT box(NULL::circle)` | `box` |
| pg_proc | `SELECT box(NULL::point)` | `box` |
| pg_proc | `SELECT box(NULL::point, NULL::point)` | `box` |
| pg_proc | `SELECT box(NULL::polygon)` | `box` |
| pg_proc | `SELECT brin_desummarize_range(NULL::regclass, NULL::bigint)` | `void` |
| pg_proc | `SELECT brin_summarize_new_values(NULL::regclass)` | `int4` |
| pg_proc | `SELECT brin_summarize_range(NULL::regclass, NULL::bigint)` | `int4` |
| pg_proc | `SELECT center(NULL::box)` | `point` |
| pg_proc | `SELECT center(NULL::circle)` | `point` |
| pg_proc | `SELECT character_length(NULL::character)` | `int4` |
| pg_proc | `SELECT character_length(NULL::text)` | `int4` |
| pg_proc | `SELECT circle(NULL::box)` | `circle` |
| pg_proc | `SELECT circle(NULL::point, NULL::double precision)` | `circle` |
| pg_proc | `SELECT circle(NULL::polygon)` | `circle` |
| pg_proc | `SELECT convert(NULL::bytea, NULL::name, NULL::name)` | `bytea` |
| pg_proc | `SELECT current_query()` | `text` |
| pg_proc | `SELECT current_schemas(NULL::boolean)` | `name[]` |
| pg_proc | `SELECT cursor_to_xml(NULL::refcursor, NULL::integer, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT cursor_to_xmlschema(NULL::refcursor, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT database_to_xml(NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT database_to_xml_and_xmlschema(NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT database_to_xmlschema(NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT diagonal(NULL::box)` | `lseg` |
| pg_proc | `SELECT diameter(NULL::circle)` | `float8` |
| pg_proc | `SELECT div(NULL::numeric, NULL::numeric)` | `numeric` |
| pg_proc | `SELECT gcd(NULL::bigint, NULL::bigint)` | `int8` |
| pg_proc | `SELECT gcd(NULL::integer, NULL::integer)` | `int4` |
| pg_proc | `SELECT gcd(NULL::numeric, NULL::numeric)` | `numeric` |
| pg_proc | `SELECT generate_subscripts(NULL::int4[], NULL::integer)` | `int4` |
| pg_proc | `SELECT generate_subscripts(NULL::int4[], NULL::integer, NULL::boolean)` | `int4` |
| pg_proc | `SELECT get_current_ts_config()` | `regconfig` |
| pg_proc | `SELECT gin_clean_pending_list(NULL::regclass)` | `int8` |
| pg_proc | `SELECT has_any_column_privilege(NULL::name, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_any_column_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_any_column_privilege(NULL::oid, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_any_column_privilege(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_any_column_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_any_column_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_foreign_data_wrapper_privilege(NULL::name, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_foreign_data_wrapper_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_foreign_data_wrapper_privilege(NULL::oid, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_foreign_data_wrapper_privilege(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_foreign_data_wrapper_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_foreign_data_wrapper_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_language_privilege(NULL::name, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_language_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_language_privilege(NULL::oid, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_language_privilege(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_language_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_language_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_parameter_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_parameter_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_parameter_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_server_privilege(NULL::name, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_server_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_server_privilege(NULL::oid, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_server_privilege(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_server_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_server_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_tablespace_privilege(NULL::name, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_tablespace_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_tablespace_privilege(NULL::oid, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_tablespace_privilege(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_tablespace_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_tablespace_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_type_privilege(NULL::name, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_type_privilege(NULL::name, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_type_privilege(NULL::oid, NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_type_privilege(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT has_type_privilege(NULL::oid, NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT has_type_privilege(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT height(NULL::box)` | `float8` |
| pg_proc | `SELECT icu_unicode_version()` | `text` |
| pg_proc | `SELECT inet_same_family(NULL::inet, NULL::inet)` | `bool` |
| pg_proc | `SELECT isclosed(NULL::path)` | `bool` |
| pg_proc | `SELECT isopen(NULL::path)` | `bool` |
| pg_proc | `SELECT json_agg_strict(NULL::int4)` | `json` |
| pg_proc | `SELECT json_object_agg_strict(NULL::int4, NULL::int4)` | `json` |
| pg_proc | `SELECT json_object_agg_unique(NULL::int4, NULL::int4)` | `json` |
| pg_proc | `SELECT json_object_agg_unique_strict(NULL::int4, NULL::int4)` | `json` |
| pg_proc | `SELECT json_populate_record(NULL::int4, NULL::json, NULL::boolean)` | `int4` |
| pg_proc | `SELECT json_populate_recordset(NULL::int4, NULL::json, NULL::boolean)` | `int4` |
| pg_proc | `SELECT json_to_record(NULL::json)` | `record` |
| pg_proc | `SELECT json_to_recordset(NULL::json)` | `record` |
| pg_proc | `SELECT json_to_tsvector(NULL::json, NULL::jsonb)` | `tsvector` |
| pg_proc | `SELECT json_to_tsvector(NULL::regconfig, NULL::json, NULL::jsonb)` | `tsvector` |
| pg_proc | `SELECT jsonb_agg_strict(NULL::int4)` | `jsonb` |
| pg_proc | `SELECT jsonb_object_agg_strict(NULL::int4, NULL::int4)` | `jsonb` |
| pg_proc | `SELECT jsonb_object_agg_unique(NULL::int4, NULL::int4)` | `jsonb` |
| pg_proc | `SELECT jsonb_object_agg_unique_strict(NULL::int4, NULL::int4)` | `jsonb` |
| pg_proc | `SELECT jsonb_populate_record(NULL::int4, NULL::jsonb)` | `int4` |
| pg_proc | `SELECT jsonb_populate_record_valid(NULL::int4, NULL::jsonb)` | `bool` |
| pg_proc | `SELECT jsonb_populate_recordset(NULL::int4, NULL::jsonb)` | `int4` |
| pg_proc | `SELECT jsonb_set_lax(NULL::jsonb, NULL::text[], NULL::jsonb, NULL::boolean, NULL::text)` | `jsonb` |
| pg_proc | `SELECT jsonb_to_record(NULL::jsonb)` | `record` |
| pg_proc | `SELECT jsonb_to_recordset(NULL::jsonb)` | `record` |
| pg_proc | `SELECT jsonb_to_tsvector(NULL::jsonb, NULL::jsonb)` | `tsvector` |
| pg_proc | `SELECT jsonb_to_tsvector(NULL::regconfig, NULL::jsonb, NULL::jsonb)` | `tsvector` |
| pg_proc | `SELECT lcm(NULL::bigint, NULL::bigint)` | `int8` |
| pg_proc | `SELECT lcm(NULL::integer, NULL::integer)` | `int4` |
| pg_proc | `SELECT lcm(NULL::numeric, NULL::numeric)` | `numeric` |
| pg_proc | `SELECT line(NULL::point, NULL::point)` | `line` |
| pg_proc | `SELECT lower(NULL::int4multirange)` | `int4` |
| pg_proc | `SELECT lseg(NULL::box)` | `lseg` |
| pg_proc | `SELECT lseg(NULL::point, NULL::point)` | `lseg` |
| pg_proc | `SELECT macaddr8_set7bit(NULL::macaddr8)` | `macaddr8` |
| pg_proc | `SELECT makeaclitem(NULL::oid, NULL::oid, NULL::text, NULL::boolean)` | `aclitem` |
| pg_proc | `SELECT max(NULL::pg_lsn)` | `pg_lsn` |
| pg_proc | `SELECT min(NULL::pg_lsn)` | `pg_lsn` |
| pg_proc | `SELECT multirange(NULL::int4range)` | `int4multirange` |
| pg_proc | `SELECT mxid_age(NULL::xid)` | `int4` |
| pg_proc | `SELECT npoints(NULL::path)` | `int4` |
| pg_proc | `SELECT npoints(NULL::polygon)` | `int4` |
| pg_proc | `SELECT numnode(NULL::tsquery)` | `int4` |
| pg_proc | `SELECT path(NULL::polygon)` | `path` |
| pg_proc | `SELECT pclose(NULL::path)` | `path` |
| pg_proc | `SELECT pg_available_wal_summaries()` | `record` |
| pg_proc | `SELECT pg_backup_start(NULL::text, NULL::boolean)` | `pg_lsn` |
| pg_proc | `SELECT pg_backup_stop(NULL::boolean)` | `record` |
| pg_proc | `SELECT pg_basetype(NULL::regtype)` | `regtype` |
| pg_proc | `SELECT pg_blocking_pids(NULL::integer)` | `int4[]` |
| pg_proc | `SELECT pg_char_to_encoding(NULL::name)` | `int4` |
| pg_proc | `SELECT pg_collation_actual_version(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_collation_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_column_compression(NULL::int4)` | `text` |
| pg_proc | `SELECT pg_column_toast_chunk_id(NULL::int4)` | `oid` |
| pg_proc | `SELECT pg_conf_load_time()` | `timestamptz` |
| pg_proc | `SELECT pg_control_checkpoint()` | `record` |
| pg_proc | `SELECT pg_control_init()` | `record` |
| pg_proc | `SELECT pg_control_recovery()` | `record` |
| pg_proc | `SELECT pg_control_system()` | `record` |
| pg_proc | `SELECT pg_conversion_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_copy_logical_replication_slot(NULL::name, NULL::name)` | `record` |
| pg_proc | `SELECT pg_copy_logical_replication_slot(NULL::name, NULL::name, NULL::boolean)` | `record` |
| pg_proc | `SELECT pg_copy_logical_replication_slot(NULL::name, NULL::name, NULL::boolean, NULL::name)` | `record` |
| pg_proc | `SELECT pg_copy_physical_replication_slot(NULL::name, NULL::name)` | `record` |
| pg_proc | `SELECT pg_copy_physical_replication_slot(NULL::name, NULL::name, NULL::boolean)` | `record` |
| pg_proc | `SELECT pg_create_logical_replication_slot(NULL::name, NULL::name, NULL::boolean, NULL::boolean, NULL::boolean)` | `record` |
| pg_proc | `SELECT pg_create_physical_replication_slot(NULL::name, NULL::boolean, NULL::boolean)` | `record` |
| pg_proc | `SELECT pg_create_restore_point(NULL::text)` | `pg_lsn` |
| pg_proc | `SELECT pg_current_logfile()` | `text` |
| pg_proc | `SELECT pg_current_logfile(NULL::text)` | `text` |
| pg_proc | `SELECT pg_current_snapshot()` | `pg_snapshot` |
| pg_proc | `SELECT pg_current_wal_flush_lsn()` | `pg_lsn` |
| pg_proc | `SELECT pg_current_wal_insert_lsn()` | `pg_lsn` |
| pg_proc | `SELECT pg_current_wal_lsn()` | `pg_lsn` |
| pg_proc | `SELECT pg_current_xact_id()` | `xid8` |
| pg_proc | `SELECT pg_current_xact_id_if_assigned()` | `xid8` |
| pg_proc | `SELECT pg_database_collation_actual_version(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_describe_object(NULL::oid, NULL::oid, NULL::integer)` | `text` |
| pg_proc | `SELECT pg_drop_replication_slot(NULL::name)` | `void` |
| pg_proc | `SELECT pg_encoding_to_char(NULL::integer)` | `name` |
| pg_proc | `SELECT pg_event_trigger_ddl_commands()` | `record` |
| pg_proc | `SELECT pg_event_trigger_dropped_objects()` | `record` |
| pg_proc | `SELECT pg_event_trigger_table_rewrite_oid()` | `oid` |
| pg_proc | `SELECT pg_event_trigger_table_rewrite_reason()` | `int4` |
| pg_proc | `SELECT pg_export_snapshot()` | `text` |
| pg_proc | `SELECT pg_filenode_relation(NULL::oid, NULL::oid)` | `regclass` |
| pg_proc | `SELECT pg_function_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_get_catalog_foreign_keys()` | `record` |
| pg_proc | `SELECT pg_get_constraintdef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_constraintdef(NULL::oid, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_expr(NULL::pg_node_tree, NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_expr(NULL::pg_node_tree, NULL::oid, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_function_arguments(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_function_identity_arguments(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_function_result(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_functiondef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_indexdef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_indexdef(NULL::oid, NULL::integer, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_keywords()` | `record` |
| pg_proc | `SELECT pg_get_multixact_members(NULL::xid)` | `record` |
| pg_proc | `SELECT pg_get_object_address(NULL::text, NULL::text[], NULL::text[])` | `record` |
| pg_proc | `SELECT pg_get_partition_constraintdef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_partkeydef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_ruledef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_ruledef(NULL::oid, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_serial_sequence(NULL::text, NULL::text)` | `text` |
| pg_proc | `SELECT pg_get_statisticsobjdef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_triggerdef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_triggerdef(NULL::oid, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_viewdef(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_get_viewdef(NULL::oid, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_viewdef(NULL::oid, NULL::integer)` | `text` |
| pg_proc | `SELECT pg_get_viewdef(NULL::text)` | `text` |
| pg_proc | `SELECT pg_get_viewdef(NULL::text, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_get_wal_replay_pause_state()` | `text` |
| pg_proc | `SELECT pg_get_wal_resource_managers()` | `record` |
| pg_proc | `SELECT pg_get_wal_summarizer_state()` | `record` |
| pg_proc | `SELECT pg_identify_object(NULL::oid, NULL::oid, NULL::integer)` | `record` |
| pg_proc | `SELECT pg_identify_object_as_address(NULL::oid, NULL::oid, NULL::integer)` | `record` |
| pg_proc | `SELECT pg_import_system_collations(NULL::regnamespace)` | `int4` |
| pg_proc | `SELECT pg_index_column_has_property(NULL::regclass, NULL::integer, NULL::text)` | `bool` |
| pg_proc | `SELECT pg_index_has_property(NULL::regclass, NULL::text)` | `bool` |
| pg_proc | `SELECT pg_indexam_has_property(NULL::oid, NULL::text)` | `bool` |
| pg_proc | `SELECT pg_input_error_info(NULL::text, NULL::text)` | `record` |
| pg_proc | `SELECT pg_input_is_valid(NULL::text, NULL::text)` | `bool` |
| pg_proc | `SELECT pg_is_other_temp_schema(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_is_wal_replay_paused()` | `bool` |
| pg_proc | `SELECT pg_jit_available()` | `bool` |
| pg_proc | `SELECT pg_last_committed_xact()` | `record` |
| pg_proc | `SELECT pg_last_wal_receive_lsn()` | `pg_lsn` |
| pg_proc | `SELECT pg_last_wal_replay_lsn()` | `pg_lsn` |
| pg_proc | `SELECT pg_last_xact_replay_timestamp()` | `timestamptz` |
| pg_proc | `SELECT pg_listening_channels()` | `text` |
| pg_proc | `SELECT pg_log_backend_memory_contexts(NULL::integer)` | `bool` |
| pg_proc | `SELECT pg_log_standby_snapshot()` | `pg_lsn` |
| pg_proc | `SELECT pg_logical_emit_message(NULL::boolean, NULL::text, NULL::bytea, NULL::boolean)` | `pg_lsn` |
| pg_proc | `SELECT pg_logical_emit_message(NULL::boolean, NULL::text, NULL::text, NULL::boolean)` | `pg_lsn` |
| pg_proc | `SELECT pg_ls_archive_statusdir()` | `record` |
| pg_proc | `SELECT pg_ls_dir(NULL::text)` | `text` |
| pg_proc | `SELECT pg_ls_dir(NULL::text, NULL::boolean, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_ls_logdir()` | `record` |
| pg_proc | `SELECT pg_ls_logicalmapdir()` | `record` |
| pg_proc | `SELECT pg_ls_logicalsnapdir()` | `record` |
| pg_proc | `SELECT pg_ls_replslotdir(NULL::text)` | `record` |
| pg_proc | `SELECT pg_ls_tmpdir()` | `record` |
| pg_proc | `SELECT pg_ls_tmpdir(NULL::oid)` | `record` |
| pg_proc | `SELECT pg_ls_waldir()` | `record` |
| pg_proc | `SELECT pg_mcv_list_items(NULL::pg_mcv_list)` | `record` |
| pg_proc | `SELECT pg_my_temp_schema()` | `oid` |
| pg_proc | `SELECT pg_notification_queue_usage()` | `float8` |
| pg_proc | `SELECT pg_opclass_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_operator_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_opfamily_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_options_to_table(NULL::text[])` | `record` |
| pg_proc | `SELECT pg_partition_ancestors(NULL::regclass)` | `regclass` |
| pg_proc | `SELECT pg_partition_root(NULL::regclass)` | `regclass` |
| pg_proc | `SELECT pg_partition_tree(NULL::regclass)` | `record` |
| pg_proc | `SELECT pg_promote(NULL::boolean, NULL::integer)` | `bool` |
| pg_proc | `SELECT pg_read_binary_file(NULL::text)` | `bytea` |
| pg_proc | `SELECT pg_read_binary_file(NULL::text, NULL::bigint, NULL::bigint)` | `bytea` |
| pg_proc | `SELECT pg_read_binary_file(NULL::text, NULL::bigint, NULL::bigint, NULL::boolean)` | `bytea` |
| pg_proc | `SELECT pg_read_binary_file(NULL::text, NULL::boolean)` | `bytea` |
| pg_proc | `SELECT pg_read_file(NULL::text)` | `text` |
| pg_proc | `SELECT pg_read_file(NULL::text, NULL::bigint, NULL::bigint)` | `text` |
| pg_proc | `SELECT pg_read_file(NULL::text, NULL::bigint, NULL::bigint, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_read_file(NULL::text, NULL::boolean)` | `text` |
| pg_proc | `SELECT pg_relation_filenode(NULL::regclass)` | `oid` |
| pg_proc | `SELECT pg_relation_filepath(NULL::regclass)` | `text` |
| pg_proc | `SELECT pg_reload_conf()` | `bool` |
| pg_proc | `SELECT pg_replication_origin_advance(NULL::text, NULL::pg_lsn)` | `void` |
| pg_proc | `SELECT pg_replication_origin_create(NULL::text)` | `oid` |
| pg_proc | `SELECT pg_replication_origin_drop(NULL::text)` | `void` |
| pg_proc | `SELECT pg_replication_origin_oid(NULL::text)` | `oid` |
| pg_proc | `SELECT pg_replication_origin_progress(NULL::text, NULL::boolean)` | `pg_lsn` |
| pg_proc | `SELECT pg_replication_origin_session_is_setup()` | `bool` |
| pg_proc | `SELECT pg_replication_origin_session_progress(NULL::boolean)` | `pg_lsn` |
| pg_proc | `SELECT pg_replication_origin_session_reset()` | `void` |
| pg_proc | `SELECT pg_replication_origin_session_setup(NULL::text)` | `void` |
| pg_proc | `SELECT pg_replication_origin_xact_reset()` | `void` |
| pg_proc | `SELECT pg_replication_origin_xact_setup(NULL::pg_lsn, NULL::timestamp with time zone)` | `void` |
| pg_proc | `SELECT pg_replication_slot_advance(NULL::name, NULL::pg_lsn)` | `record` |
| pg_proc | `SELECT pg_rotate_logfile()` | `bool` |
| pg_proc | `SELECT pg_safe_snapshot_blocking_pids(NULL::integer)` | `int4[]` |
| pg_proc | `SELECT pg_settings_get_flags(NULL::text)` | `text[]` |
| pg_proc | `SELECT pg_size_bytes(NULL::text)` | `int8` |
| pg_proc | `SELECT pg_sleep_for(NULL::interval)` | `void` |
| pg_proc | `SELECT pg_sleep_until(NULL::timestamp with time zone)` | `void` |
| pg_proc | `SELECT pg_snapshot_xip(NULL::pg_snapshot)` | `xid8` |
| pg_proc | `SELECT pg_snapshot_xmax(NULL::pg_snapshot)` | `xid8` |
| pg_proc | `SELECT pg_snapshot_xmin(NULL::pg_snapshot)` | `xid8` |
| pg_proc | `SELECT pg_split_walfile_name(NULL::text)` | `record` |
| pg_proc | `SELECT pg_stat_file(NULL::text)` | `record` |
| pg_proc | `SELECT pg_stat_file(NULL::text, NULL::boolean)` | `record` |
| pg_proc | `SELECT pg_statistics_obj_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_switch_wal()` | `pg_lsn` |
| pg_proc | `SELECT pg_sync_replication_slots()` | `void` |
| pg_proc | `SELECT pg_table_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_tablespace_databases(NULL::oid)` | `oid` |
| pg_proc | `SELECT pg_tablespace_location(NULL::oid)` | `text` |
| pg_proc | `SELECT pg_tablespace_size(NULL::name)` | `int8` |
| pg_proc | `SELECT pg_tablespace_size(NULL::oid)` | `int8` |
| pg_proc | `SELECT pg_trigger_depth()` | `int4` |
| pg_proc | `SELECT pg_ts_config_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_ts_dict_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_ts_parser_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_ts_template_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_type_is_visible(NULL::oid)` | `bool` |
| pg_proc | `SELECT pg_visible_in_snapshot(NULL::xid8, NULL::pg_snapshot)` | `bool` |
| pg_proc | `SELECT pg_wal_lsn_diff(NULL::pg_lsn, NULL::pg_lsn)` | `numeric` |
| pg_proc | `SELECT pg_wal_replay_pause()` | `void` |
| pg_proc | `SELECT pg_wal_replay_resume()` | `void` |
| pg_proc | `SELECT pg_wal_summary_contents(NULL::bigint, NULL::pg_lsn, NULL::pg_lsn)` | `record` |
| pg_proc | `SELECT pg_walfile_name(NULL::pg_lsn)` | `text` |
| pg_proc | `SELECT pg_walfile_name_offset(NULL::pg_lsn)` | `record` |
| pg_proc | `SELECT pg_xact_commit_timestamp(NULL::xid)` | `timestamptz` |
| pg_proc | `SELECT pg_xact_commit_timestamp_origin(NULL::xid)` | `record` |
| pg_proc | `SELECT pg_xact_status(NULL::xid8)` | `text` |
| pg_proc | `SELECT point(NULL::box)` | `point` |
| pg_proc | `SELECT point(NULL::circle)` | `point` |
| pg_proc | `SELECT point(NULL::double precision, NULL::double precision)` | `point` |
| pg_proc | `SELECT point(NULL::lseg)` | `point` |
| pg_proc | `SELECT point(NULL::polygon)` | `point` |
| pg_proc | `SELECT polygon(NULL::box)` | `polygon` |
| pg_proc | `SELECT polygon(NULL::circle)` | `polygon` |
| pg_proc | `SELECT polygon(NULL::integer, NULL::circle)` | `polygon` |
| pg_proc | `SELECT polygon(NULL::path)` | `polygon` |
| pg_proc | `SELECT popen(NULL::path)` | `path` |
| pg_proc | `SELECT query_to_xml(NULL::text, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT query_to_xml_and_xmlschema(NULL::text, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT query_to_xmlschema(NULL::text, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT querytree(NULL::tsquery)` | `text` |
| pg_proc | `SELECT radius(NULL::circle)` | `float8` |
| pg_proc | `SELECT range_agg(NULL::int4multirange)` | `int4multirange` |
| pg_proc | `SELECT range_agg(NULL::int4range)` | `int4multirange` |
| pg_proc | `SELECT range_intersect_agg(NULL::int4multirange)` | `int4multirange` |
| pg_proc | `SELECT range_intersect_agg(NULL::int4range)` | `int4range` |
| pg_proc | `SELECT range_merge(NULL::int4multirange)` | `int4range` |
| pg_proc | `SELECT range_merge(NULL::int4range, NULL::int4range)` | `int4range` |
| pg_proc | `SELECT row_security_active(NULL::oid)` | `bool` |
| pg_proc | `SELECT row_security_active(NULL::text)` | `bool` |
| pg_proc | `SELECT schema_to_xml(NULL::name, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT schema_to_xml_and_xmlschema(NULL::name, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT schema_to_xmlschema(NULL::name, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT shobj_description(NULL::oid, NULL::name)` | `text` |
| pg_proc | `SELECT slope(NULL::point, NULL::point)` | `float8` |
| pg_proc | `SELECT strip(NULL::tsvector)` | `tsvector` |
| pg_proc | `SELECT suppress_redundant_updates_trigger()` | `trigger` |
| pg_proc | `SELECT table_to_xml(NULL::regclass, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT table_to_xml_and_xmlschema(NULL::regclass, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT table_to_xmlschema(NULL::regclass, NULL::boolean, NULL::boolean, NULL::text)` | `xml` |
| pg_proc | `SELECT timezone(NULL::time with time zone)` | `timetz` |
| pg_proc | `SELECT timezone(NULL::timestamp with time zone)` | `timestamp` |
| pg_proc | `SELECT timezone(NULL::timestamp without time zone)` | `timestamptz` |
| pg_proc | `SELECT to_ascii(NULL::text)` | `text` |
| pg_proc | `SELECT to_ascii(NULL::text, NULL::integer)` | `text` |
| pg_proc | `SELECT to_ascii(NULL::text, NULL::name)` | `text` |
| pg_proc | `SELECT to_regclass(NULL::text)` | `regclass` |
| pg_proc | `SELECT to_regcollation(NULL::text)` | `regcollation` |
| pg_proc | `SELECT to_regnamespace(NULL::text)` | `regnamespace` |
| pg_proc | `SELECT to_regoper(NULL::text)` | `regoper` |
| pg_proc | `SELECT to_regoperator(NULL::text)` | `regoperator` |
| pg_proc | `SELECT to_regproc(NULL::text)` | `regproc` |
| pg_proc | `SELECT to_regprocedure(NULL::text)` | `regprocedure` |
| pg_proc | `SELECT to_regrole(NULL::text)` | `regrole` |
| pg_proc | `SELECT to_regtype(NULL::text)` | `regtype` |
| pg_proc | `SELECT to_regtypemod(NULL::text)` | `int4` |
| pg_proc | `SELECT trunc(NULL::macaddr)` | `macaddr` |
| pg_proc | `SELECT trunc(NULL::macaddr8)` | `macaddr8` |
| pg_proc | `SELECT ts_debug(NULL::regconfig, NULL::text)` | `record` |
| pg_proc | `SELECT ts_debug(NULL::text)` | `record` |
| pg_proc | `SELECT ts_delete(NULL::tsvector, NULL::text)` | `tsvector` |
| pg_proc | `SELECT ts_delete(NULL::tsvector, NULL::text[])` | `tsvector` |
| pg_proc | `SELECT ts_filter(NULL::tsvector, NULL::"char"[])` | `tsvector` |
| pg_proc | `SELECT ts_lexize(NULL::regdictionary, NULL::text)` | `text[]` |
| pg_proc | `SELECT ts_parse(NULL::oid, NULL::text)` | `record` |
| pg_proc | `SELECT ts_parse(NULL::text, NULL::text)` | `record` |
| pg_proc | `SELECT ts_rewrite(NULL::tsquery, NULL::text)` | `tsquery` |
| pg_proc | `SELECT ts_rewrite(NULL::tsquery, NULL::tsquery, NULL::tsquery)` | `tsquery` |
| pg_proc | `SELECT ts_stat(NULL::text)` | `record` |
| pg_proc | `SELECT ts_stat(NULL::text, NULL::text)` | `record` |
| pg_proc | `SELECT ts_token_type(NULL::oid)` | `record` |
| pg_proc | `SELECT ts_token_type(NULL::text)` | `record` |
| pg_proc | `SELECT tsquery_phrase(NULL::tsquery, NULL::tsquery)` | `tsquery` |
| pg_proc | `SELECT tsquery_phrase(NULL::tsquery, NULL::tsquery, NULL::integer)` | `tsquery` |
| pg_proc | `SELECT tsvector_to_array(NULL::tsvector)` | `text[]` |
| pg_proc | `SELECT tsvector_update_trigger()` | `trigger` |
| pg_proc | `SELECT tsvector_update_trigger_column()` | `trigger` |
| pg_proc | `SELECT txid_current_if_assigned()` | `int8` |
| pg_proc | `SELECT txid_current_snapshot()` | `txid_snapshot` |
| pg_proc | `SELECT txid_snapshot_xip(NULL::txid_snapshot)` | `int8` |
| pg_proc | `SELECT txid_snapshot_xmax(NULL::txid_snapshot)` | `int8` |
| pg_proc | `SELECT txid_snapshot_xmin(NULL::txid_snapshot)` | `int8` |
| pg_proc | `SELECT txid_status(NULL::bigint)` | `text` |
| pg_proc | `SELECT txid_visible_in_snapshot(NULL::bigint, NULL::txid_snapshot)` | `bool` |
| pg_proc | `SELECT unicode_assigned(NULL::text)` | `bool` |
| pg_proc | `SELECT unicode_version()` | `text` |
| pg_proc | `SELECT unnest(NULL::int4multirange)` | `int4range` |
| pg_proc | `SELECT unnest(NULL::tsvector)` | `record` |
| pg_proc | `SELECT upper(NULL::int4multirange)` | `int4` |
| pg_proc | `SELECT width(NULL::box)` | `float8` |
| pg_proc | `SELECT xml_is_well_formed(NULL::text)` | `bool` |
| pg_proc | `SELECT xml_is_well_formed_content(NULL::text)` | `bool` |
| pg_proc | `SELECT xml_is_well_formed_document(NULL::text)` | `bool` |
| pg_proc | `SELECT xmlagg(NULL::xml)` | `xml` |
| pg_proc | `SELECT xmlcomment(NULL::text)` | `xml` |
| pg_proc | `SELECT xmltext(NULL::text)` | `xml` |
| pg_proc | `SELECT xpath(NULL::text, NULL::xml)` | `xml[]` |
| pg_proc | `SELECT xpath(NULL::text, NULL::xml, NULL::text[])` | `xml[]` |
| pg_proc | `SELECT xpath_exists(NULL::text, NULL::xml)` | `bool` |
| pg_proc | `SELECT xpath_exists(NULL::text, NULL::xml, NULL::text[])` | `bool` |
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
| pg_operator | `SELECT NULL::int4range * NULL::int4range` | `int4range` |
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
| pg_operator | `SELECT NULL::int4range + NULL::int4range` | `int4range` |
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
| pg_operator | `SELECT NULL::int4range - NULL::int4range` | `int4range` |
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
