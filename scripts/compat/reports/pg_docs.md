# PostgreSQL 17 functions and operators

2553 queries, `scripts/compat/pg_docs.py`: the examples of the documentation's function and operator tables, each `pg_proc` overload of a documented function, and each `pg_operator` operator. 2514 columns agree with the server.

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

None.

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
