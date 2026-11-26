# Tenant scope

## With a tenancy

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.tenancy]
table = "public.orgs"
exclude = ["audit_log.org_id"]
columns = ["events.org"]
```

`schema.sql`:

```sql
CREATE TABLE orgs (id SERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES orgs, name TEXT NOT NULL, UNIQUE (org_id, id));
CREATE TABLE invoices (
    id SERIAL PRIMARY KEY,
    org_id INTEGER NOT NULL REFERENCES orgs,
    project_id INTEGER NOT NULL,
    FOREIGN KEY (org_id, project_id) REFERENCES projects (org_id, id)
);
CREATE TABLE audit_log (id SERIAL PRIMARY KEY, org_id INTEGER REFERENCES orgs, message TEXT NOT NULL);
CREATE TABLE events (id BIGSERIAL PRIMARY KEY, org INTEGER NOT NULL, payload JSONB);
CREATE TABLE transfers (id SERIAL PRIMARY KEY, from_org INTEGER NOT NULL REFERENCES orgs, to_org INTEGER NOT NULL REFERENCES orgs);
CREATE TABLE secrets (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES orgs, value TEXT NOT NULL);
ALTER TABLE secrets ENABLE ROW LEVEL SECURITY;
ALTER TABLE secrets FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON secrets USING (org_id = current_setting('app.org_id')::int);
CREATE TABLE tags (id SERIAL PRIMARY KEY, name TEXT NOT NULL);
```

### The tenant table is scoped by its primary key

```python
from sqlty import SQL

# error: [tenant-scope] "`orgs` is read without `orgs.id = <parameter>`"
all_orgs = SQL[tuple[str]]("--sql\nSELECT name FROM orgs")

# error:
one = SQL[tuple[str]]("--sql\nSELECT name FROM orgs WHERE id = $1")
```

### Parameters

`IN` over parameters, `= ANY` of one, casts and named parameters pin the key.

```python
from sqlty import SQL

# error:
listed = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id IN (%s, %s)")

# error:
any_of = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = ANY(%s)")

# error:
cast = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = %(org)s::INT")

# error:
reversed = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE %s = org_id")
```

A constant, a comparison other than `=`, and a pin under `OR` do not.

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
constant = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = 1")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
greater = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id > %s")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
either = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = %s OR name = %s")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
mixed = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id IN (%s, 2)")
```

### Joins

A pinned key passes through `=` and `USING`, and the tenant table pins the tables
joined to its primary key.

```python
from sqlty import SQL

# error:
through_orgs = SQL[tuple[str]]("--sql\nSELECT p.name FROM orgs o JOIN projects p ON p.org_id = o.id WHERE o.id = %s")

# error:
using = SQL[tuple[int]]("--sql\nSELECT i.id FROM invoices i JOIN projects p USING (org_id) WHERE p.org_id = %s")

# error:
chained = SQL[tuple[int]]("--sql\nSELECT i.id FROM invoices i, projects p, orgs o WHERE i.org_id = p.org_id AND p.org_id = o.id AND o.id = %s")
```

A `LEFT JOIN`'s `ON` pins the joined table, not the one it keeps every row of.

```python
from sqlty import SQL

# error:
left = SQL[tuple[str, int | None]]("--sql\nSELECT p.name, i.id FROM projects p LEFT JOIN invoices i ON i.org_id = p.org_id WHERE p.org_id = %s")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
kept = SQL[tuple[str, int | None]]("--sql\nSELECT p.name, i.id FROM projects p LEFT JOIN invoices i ON i.org_id = p.org_id AND i.org_id = %s")

# error: [tenant-scope] "`invoices` is read without `invoices.org_id = <parameter>`"
right = SQL[tuple[str | None, int]]("--sql\nSELECT p.name, i.id FROM projects p RIGHT JOIN invoices i ON i.org_id = p.org_id AND p.org_id = %s")
```

### Each table is judged

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
# error: [tenant-scope] "`invoices` is read without `invoices.org_id = <parameter>`"
neither = SQL[tuple[int]]("--sql\nSELECT i.id FROM invoices i JOIN projects p ON p.org_id = i.org_id AND p.id = i.project_id")
```

### Two tenant keys

Either key pins the table.

```python
from sqlty import SQL

# error:
outgoing = SQL[tuple[int]]("--sql\nSELECT id FROM transfers WHERE from_org = %s")

# error: [tenant-scope] "`transfers` is read without `transfers.from_org = <parameter>` or `transfers.to_org = <parameter>`"
every = SQL[tuple[int]]("--sql\nSELECT id FROM transfers")
```

### Tables that are not judged

Tables without a tenant key, excluded keys and tables with row-level security.

```python
from sqlty import SQL

# error:
tags = SQL[tuple[str]]("--sql\nSELECT name FROM tags")

# error:
audit = SQL[tuple[str]]("--sql\nSELECT message FROM audit_log")

# error:
secret = SQL[tuple[str]]("--sql\nSELECT value FROM secrets")
```

### Subqueries

A subquery is judged on its own; a correlated one is pinned by the enclosing row's
tenant key.

```python
from sqlty import SQL

# error:
correlated = SQL[tuple[str]]("--sql\nSELECT p.name FROM projects p WHERE p.org_id = %s AND EXISTS (SELECT 1 FROM invoices i WHERE i.org_id = p.org_id)")

# error: [tenant-scope] "`invoices` is read without `invoices.org_id = <parameter>`"
loose = SQL[tuple[str]]("--sql\nSELECT p.name FROM projects p WHERE p.org_id = %s AND (p.org_id, p.id) IN (SELECT org_id, project_id FROM invoices)")
```

A CTE is judged by its body.

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
outside = SQL[tuple[str]]("--sql\nWITH p AS (SELECT name, org_id FROM projects) SELECT name FROM p WHERE org_id = %s")

# error:
inside = SQL[tuple[str]]("--sql\nWITH p AS (SELECT name FROM projects WHERE org_id = %s) SELECT name FROM p")
```

### UPDATE and DELETE

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is updated without `projects.org_id = <parameter>`"
rename = SQL("--sql\nUPDATE projects SET name = %s WHERE id = %s")

# error:
scoped = SQL("--sql\nUPDATE projects SET name = %s WHERE id = %s AND org_id = %s")

# error: [tenant-scope] "`invoices` is deleted from without `invoices.org_id = <parameter>`"
purge = SQL("--sql\nDELETE FROM invoices USING projects p WHERE invoices.project_id = p.id AND p.org_id = %s")

# error:
joined = SQL("--sql\nDELETE FROM invoices USING projects p WHERE invoices.org_id = p.org_id AND p.org_id = %s")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
from_unscoped = SQL("--sql\nUPDATE invoices SET project_id = p.id FROM projects p WHERE invoices.org_id = %s AND p.name = %s")
```

### INSERT

```python
from sqlty import SQL

# error:
created = SQL("--sql\nINSERT INTO projects (org_id, name) VALUES (%s, %s), ($1, %s)")

# error: [tenant-scope] "INSERT into `projects` does not set `org_id` from a parameter"
second_row = SQL("--sql\nINSERT INTO projects (org_id, name) VALUES (%s, %s), (2, %s)")

# error: [insert-missing-column]
# error: [tenant-scope] "INSERT into `projects` does not set `org_id` from a parameter"
missing = SQL("--sql\nINSERT INTO projects (name) VALUES (%s)")

# error:
positional = SQL("--sql\nINSERT INTO projects VALUES (DEFAULT, %s, %s)")

# error:
new_org = SQL("--sql\nINSERT INTO orgs (name) VALUES (%s)")

# error:
copied = SQL("--sql\nINSERT INTO invoices (org_id, project_id) SELECT org_id, id FROM projects WHERE org_id = %s")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
copied_all = SQL("--sql\nINSERT INTO invoices (org_id, project_id) SELECT org_id, id FROM projects")
```

A query source sets the tenant key from its own columns, which that query pins; a
constant in its select list is reported.

```python
from sqlty import SQL

# error: [tenant-scope] "INSERT into `invoices` does not set `org_id` from a parameter"
constant = SQL("--sql\nINSERT INTO invoices (org_id, project_id) SELECT 1, id FROM projects WHERE org_id = %s")
```

An upsert or an UPDATE that moves a row to another tenant is reported.

```python
from sqlty import SQL

# error: [tenant-scope] "UPDATE of `projects` does not set `org_id` from a parameter"
upsert = SQL("--sql\nINSERT INTO projects (id, org_id, name) VALUES (%s, %s, %s) ON CONFLICT (id) DO UPDATE SET org_id = 1")

# error:
upsert_excluded = SQL("--sql\nINSERT INTO projects (id, org_id, name) VALUES (%s, %s, %s) ON CONFLICT (id) DO UPDATE SET org_id = EXCLUDED.org_id")

# error: [tenant-scope] "UPDATE of `projects` does not set `org_id` from a parameter"
moved = SQL("--sql\nUPDATE projects SET org_id = 2 WHERE org_id = %s")

# error:
handed = SQL("--sql\nUPDATE projects SET org_id = %s WHERE org_id = %s AND id = %s")
```

### Composite keys

Every column of a composite tenant key must be pinned.

```toml
[tool.sqlty]
schema = "regions.sql"

[tool.sqlty.tenancy]
table = "orgs"
```

`regions.sql`:

```sql
CREATE TABLE orgs (id INTEGER, region TEXT, parent_id INTEGER, PRIMARY KEY (id, region));
CREATE TABLE sites (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, region TEXT NOT NULL, FOREIGN KEY (org_id, region) REFERENCES orgs);
```

```python
from sqlty import SQL

# error: [tenant-scope] "`sites` is read without `sites.org_id = <parameter>` and `sites.region = <parameter>`"
by_region = SQL[tuple[int]]("--sql\nSELECT id FROM sites WHERE region = %s")

# error:
by_org = SQL[tuple[int]]("--sql\nSELECT id FROM sites WHERE org_id = %s AND region = %s")

# error: [tenant-scope] "`orgs` is read without `orgs.id = <parameter>` and `orgs.region = <parameter>`"
children = SQL[tuple[int]]("--sql\nSELECT id FROM orgs WHERE parent_id = %s")
```

### Suppressed

```python
from sqlty import SQL

# error:
report = SQL[tuple[int]]("--sql\nSELECT count(*)::INT FROM projects")  # sqlty: ignore[tenant-scope]
```

## MySQL

```toml
[tool.sqlty]
schema = "my.sql"
dialect = "mysql"

[tool.sqlty.tenancy]
table = "orgs"
```

`my.sql`:

```sql
CREATE TABLE orgs (id INT PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE projects (id INT PRIMARY KEY, org_id INT NOT NULL, name TEXT NOT NULL, FOREIGN KEY (org_id) REFERENCES orgs (id));
CREATE TABLE invoices (id INT PRIMARY KEY, org_id INT NOT NULL, FOREIGN KEY (org_id) REFERENCES orgs (id));
```

```python
from sqlty import SQL

# error:
straight = SQL[tuple[int]]("-- sql\nSELECT i.id FROM projects p STRAIGHT_JOIN invoices i ON i.org_id = p.org_id WHERE p.org_id = %s")

# error: [tenant-scope] "INSERT into `projects` does not set `org_id` from a parameter"
assigned = SQL("-- sql\nINSERT INTO projects SET id = %s, org_id = 1, name = %s")
```

## Other dialects

```toml
[tool.sqlty]
schema = "lite.sql"
dialect = "sqlite"

[tool.sqlty.tenancy]
table = "orgs"
```

`lite.sql`:

```sql
CREATE TABLE orgs (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE projects (id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES orgs (id), name TEXT NOT NULL);
```

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
names = SQL[tuple[str]]("--sql\nSELECT name FROM projects")

# error:
mine = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = ?")

# error:
named = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = :org")
```

## Without a tenancy

```toml
[tool.sqlty]
schema = "plain.sql"
```

`plain.sql`:

```sql
CREATE TABLE orgs (id SERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES orgs, name TEXT NOT NULL);
```

```python
from sqlty import SQL

# error:
names = SQL[tuple[str]]("--sql\nSELECT name FROM projects")
```

## Row-level security policies

A policy that compares a column to a session value makes the column a tenant key, with
or without `[tool.sqlty.tenancy]`. A table is left to the database only when its
policies are forced and pin a tenant key for every command.

```toml
[tool.sqlty]
schema = "rls.sql"

[tool.sqlty.tenancy]
columns = ["dropped.org_id"]
session-functions = ["auth.uid"]
```

`rls.sql`:

```sql
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, name TEXT NOT NULL);
ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE projects FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON projects USING (org_id = current_setting('app.org_id')::int);

CREATE TABLE unforced (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL);
ALTER TABLE unforced ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON unforced USING (org_id = current_setting('app.org_id')::int);

CREATE TABLE invoices (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL);
ALTER TABLE invoices ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON invoices FOR ALL USING (org_id = current_setting('app.org_id')::int) WITH CHECK (true);

CREATE TABLE notes (id SERIAL PRIMARY KEY, owner UUID NOT NULL, body TEXT);
ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE notes FORCE ROW LEVEL SECURITY;
CREATE POLICY own ON notes USING (owner = (SELECT auth.uid()));

CREATE TABLE open_to_all (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL);
ALTER TABLE open_to_all ENABLE ROW LEVEL SECURITY;
ALTER TABLE open_to_all FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON open_to_all USING (org_id = current_setting('app.org_id')::int);
CREATE POLICY everyone ON open_to_all FOR SELECT USING (true);

CREATE TABLE narrowed (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL);
ALTER TABLE narrowed ENABLE ROW LEVEL SECURITY;
ALTER TABLE narrowed FORCE ROW LEVEL SECURITY;
CREATE POLICY everyone ON narrowed USING (true);
CREATE POLICY tenant ON narrowed AS RESTRICTIVE USING (org_id = current_setting('app.org_id')::int);

CREATE TABLE dropped (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL);
ALTER TABLE dropped ENABLE ROW LEVEL SECURITY;
ALTER TABLE dropped FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON dropped USING (org_id = current_setting('app.org_id')::int);
ALTER POLICY tenant ON dropped USING (true);
```

### Forced policies that pin the key for every command

```python
from sqlty import SQL

# error:
names = SQL[tuple[str]]("--sql\nSELECT name FROM projects")

# error:
narrowed = SQL[tuple[int]]("--sql\nSELECT id FROM narrowed")
```

### Policies the database does not enforce for every role or command

The owner of a table without `FORCE ROW LEVEL SECURITY` skips its policies.
`WITH CHECK (true)` lets an `INSERT` write a row of another tenant, and a permissive
`USING (true)` for `SELECT` shows every row.

```python
from sqlty import SQL

# error: [tenant-scope] "`unforced` is read without `unforced.org_id = <parameter>`"
unforced = SQL[tuple[int]]("--sql\nSELECT id FROM unforced")

# error: [tenant-scope] "`invoices` is read without `invoices.org_id = <parameter>`"
invoices = SQL[tuple[int]]("--sql\nSELECT id FROM invoices")

# error: [tenant-scope] "`open_to_all` is read without `open_to_all.org_id = <parameter>`"
open_to_all = SQL[tuple[int]]("--sql\nSELECT id FROM open_to_all")

# error:
scoped = SQL[tuple[int]]("--sql\nSELECT id FROM unforced WHERE org_id = %s")
```

### Session functions

`session-functions` names the functions a policy reads the session's tenant from, such
as Supabase's `auth.uid()`.

```python
from sqlty import SQL

# error:
notes = SQL[tuple[str | None]]("--sql\nSELECT body FROM notes")
```

### ALTER POLICY

`ALTER POLICY` replaces the condition: the policy no longer pins `org_id`, which
`columns` makes a tenant key.

```python
from sqlty import SQL

# error: [tenant-scope] "`dropped` is read without `dropped.org_id = <parameter>`"
dropped = SQL[tuple[int]]("--sql\nSELECT id FROM dropped")
```

## Policies without a tenancy

```toml
[tool.sqlty]
schema = "policy.sql"
```

`policy.sql`:

```sql
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, name TEXT NOT NULL);
ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON projects USING (org_id = current_setting('app.org_id')::int);
ALTER TABLE projects RENAME COLUMN org_id TO tenant_id;
```

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is read without `projects.tenant_id = <parameter>`"
names = SQL[tuple[str]]("--sql\nSELECT name FROM projects")

# error:
mine = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE tenant_id = %s")
```
