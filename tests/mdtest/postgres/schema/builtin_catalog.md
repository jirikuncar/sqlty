# pg_catalog and information_schema

The common system relations are known without any schema file content.

```toml
[tool.sqlty]
dialect = "postgres"
```

## pg_catalog

`pg_catalog` is always searched, so its relations need no qualifier.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```sql
SELECT c.relname, c.relkind, c.reltuples, c.relhasindex FROM pg_class c
-- inferred: tuple[str, str, float, bool]
```

```sql
SELECT n.nspname, n.oid FROM pg_catalog.pg_namespace n
-- inferred: tuple[str, int]
```

```sql
SELECT a.attname, a.attnum, a.attnotnull
FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
WHERE c.relname = 'users'
-- inferred: tuple[str, int, bool]
```

```sql
SELECT typname, typtype, typlen FROM pg_type
-- inferred: tuple[str, str, int]
```

## information_schema

It is not on the search path: a bare `columns` is not the system view.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```sql
SELECT table_schema, table_name, table_type FROM information_schema.tables
-- inferred: tuple[str, str, str]
```

```sql
SELECT column_name, ordinal_position, is_nullable, data_type
FROM information_schema.columns WHERE table_name = 'users'
-- inferred: tuple[str, int, str, str]
```

```sql
SELECT column_name FROM columns
-- inferred: None
```

## Without a schema file

The built-in relations are there even when the project has no schema.

```sql
SELECT relname, relnatts FROM pg_class
-- inferred: tuple[str, int]
```

## Other dialects do not have them

```toml
[tool.sqlty]
dialect = "sqlite"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```sql
SELECT relname FROM pg_class
-- inferred: None
```
