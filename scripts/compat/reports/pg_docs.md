# PostgreSQL 17 functions and operators

2553 queries, `scripts/compat/pg_docs.py`: the examples of the documentation's function and operator tables, each `pg_proc` overload of a documented function, and each `pg_operator` operator. 2488 columns agree with the server.

## Typed differently

None.

## Untyped

| Source | Query | Server type |
|---|---|---|
| functions-array.html | `SELECT ARRAY[1,2,3] \|\| ARRAY[[4,5,6],[7,8,9.9]]` | `numeric[]` |
| pg_proc | `SELECT pg_event_trigger_ddl_commands()` | `record` |
| pg_proc | `SELECT pg_event_trigger_dropped_objects()` | `record` |
| pg_proc | `SELECT pg_mcv_list_items(NULL::pg_mcv_list)` | `record` |
| pg_proc | `SELECT suppress_redundant_updates_trigger()` | `trigger` |
| pg_proc | `SELECT tsvector_update_trigger()` | `trigger` |
| pg_proc | `SELECT tsvector_update_trigger_column()` | `trigger` |
| pg_operator | `SELECT NULL::aclitem[] + NULL::aclitem` | `aclitem[]` |
| pg_operator | `SELECT NULL::aclitem[] - NULL::aclitem` | `aclitem[]` |

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
