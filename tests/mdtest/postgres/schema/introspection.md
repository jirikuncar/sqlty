# Introspection queries

Typical queries over `pg_catalog` and `information_schema` get the PostgreSQL
column types: `oid` is `int`; `name`, `"char"`, `reg*`, `xid` and the vector types are `str`, as psycopg loads them. A column the catalog
declares `NOT NULL` is not optional.

```toml
[tool.sqlty]
dialect = "postgres"
```

## List indexes

```sql
SELECT c.relname, i.indisunique, i.indisprimary, i.indkey, i.indexprs, i.indpred
FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
-- inferred: tuple[str, bool, bool, str, str | None, str | None]
```

```sql
SELECT schemaname, tablename, indexname, indexdef FROM pg_indexes
-- inferred: tuple[str | None, str | None, str | None, str | None]
```

## Enums

```sql
SELECT t.typname, e.enumlabel, e.enumsortorder
FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid
ORDER BY e.enumsortorder
-- inferred: tuple[str, str, float]
```

## Constraints

```sql
SELECT conname, contype, conrelid, confrelid, conkey, confkey, conbin FROM pg_constraint
-- inferred: tuple[str, str, int, int, list[int] | None, list[int] | None, str | None]
```

```sql
SELECT tc.constraint_name, tc.constraint_type, kcu.column_name, kcu.position_in_unique_constraint
FROM information_schema.table_constraints tc
JOIN information_schema.key_column_usage kcu USING (constraint_name)
-- inferred: tuple[str | None, str | None, str | None, int | None]
```

```sql
SELECT tc.constraint_name, ccu.table_name, ccu.column_name
FROM information_schema.table_constraints tc
JOIN information_schema.constraint_column_usage ccu USING (constraint_name)
-- inferred: tuple[str | None, str | None, str | None]
```

## Columns and defaults

```sql
SELECT a.attname, a.atttypid, a.attnotnull, a.attidentity, d.adbin
FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
-- inferred: tuple[str, int, bool, str, str | None]
```

```sql
SELECT table_name, column_name, is_nullable, character_maximum_length
FROM information_schema.columns
-- inferred: tuple[str | None, str | None, str | None, int | None]
```

## Functions

```sql
SELECT proname, prokind, proargtypes, proallargtypes, proargmodes, proargnames, prosrc
FROM pg_proc
-- inferred: tuple[str, str, str, list[int] | None, list[str] | None, list[str] | None, str]
```

```sql
SELECT routine_schema, routine_name, routine_type, created FROM information_schema.routines
-- inferred: tuple[str | None, str | None, str | None, datetime.datetime | None]
```

## Dependencies and comments

```sql
SELECT classid, objid, objsubid, refobjid, deptype FROM pg_depend
-- inferred: tuple[int, int, int, int, str]
```

```sql
SELECT objoid, objsubid, description FROM pg_description
-- inferred: tuple[int, int, str]
```

## Roles, settings and activity

```sql
SELECT rolname, rolsuper, rolconnlimit, rolvaliduntil, rolconfig FROM pg_roles
-- inferred: tuple[str | None, bool | None, int | None, datetime.datetime | None, list[str] | None]
```

```sql
SELECT name, setting, vartype, enumvals, pending_restart FROM pg_settings
-- inferred: tuple[str | None, str | None, str | None, list[str] | None, bool | None]
```

```sql
SELECT pid, usename, client_addr, backend_xid, query_start, state FROM pg_stat_activity
-- inferred: tuple[int | None, str | None, ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface | None, str | None, datetime.datetime | None, str | None]
```

## Databases and statistics

```sql
SELECT datname, datcollate, datctype, datlocale, datconnlimit FROM pg_database WHERE datname = current_database()
-- inferred: tuple[str, str, str, str | None, int]
```

```sql
SELECT n_distinct, null_frac, most_common_freqs FROM pg_stats WHERE tablename = 'users' AND attname = 'id'
-- inferred: tuple[float | None, float | None, list[float] | None]
```

## Tables, views, sequences and schemas

```sql
SELECT schemaname, tablename, tableowner, hasindexes FROM pg_tables
-- inferred: tuple[str | None, str | None, str | None, bool | None]
```

```sql
SELECT viewname, definition FROM pg_views
-- inferred: tuple[str | None, str | None]
```

```sql
SELECT sequencename, data_type, increment_by, last_value FROM pg_sequences
-- inferred: tuple[str | None, str | None, int | None, int | None]
```

```sql
SELECT schema_name, schema_owner FROM information_schema.schemata
-- inferred: tuple[str | None, str | None]
```

```sql
SELECT table_name, view_definition, is_updatable FROM information_schema.views
-- inferred: tuple[str | None, str | None, str | None]
```

## Types and namespaces

```sql
SELECT n.nspname, n.nspacl, t.typtype, t.typinput, t.typacl
FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
-- inferred: tuple[str, list[str] | None, str, str, list[str] | None]
```

```sql
SELECT relkind, relfrozenxid, relacl, relpartbound FROM pg_class
-- inferred: tuple[str, str, list[str] | None, str | None]
```

## Typos and unmodelled relations

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"

[tool.sqlty.rules]
unknown-column = "error"
unsupported-syntax = "error"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```python
from sqlty import SQL

a = SQL[tuple[str, bool]]("--sql\nSELECT c.relname, i.indisunique FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid")
# error: [unknown-column] "column `indexrelid` does not exist in `pg_enum`"
b = SQL[tuple[int]]("--sql\nSELECT indexrelid FROM pg_enum")

# error: [unsupported-syntax] "column `tgrelid` may come from system catalog pg_trigger, which sqlty does not read the columns of"
c = SQL[tuple[int]]("--sql\nSELECT tgrelid FROM pg_trigger")
```

`pg_trigger` is not modelled, so its columns are unknown rather than reported.
