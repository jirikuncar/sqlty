# Catalog refresh after an unreadable function

The loader keeps a resolved catalog while it reads the schema and updates it as
statements arrive. These pin what a later view sees after such an update.

## A function whose return type cannot be read

A `CREATE OR REPLACE FUNCTION` the loader cannot read (no `RETURNS`, no `OUT`
parameters) makes the function unknown, also for views created after it.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE t (id integer NOT NULL);
CREATE FUNCTION f() RETURNS integer LANGUAGE sql AS 'SELECT 1';
CREATE VIEW before AS SELECT f() AS x, id FROM t;
CREATE OR REPLACE FUNCTION f() LANGUAGE sql AS 'SELECT 1';
CREATE VIEW after AS SELECT f() AS x, id FROM t;
```

```sql
SELECT x, id FROM before
-- inferred: tuple[int | None, int]
```

```sql
SELECT x, id FROM after
-- inferred: None
```
