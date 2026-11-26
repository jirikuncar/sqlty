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

### Suppressed

```python
from sqlty import SQL

# error:
report = SQL[tuple[int]]("--sql\nSELECT count(*)::INT FROM projects")  # sqlty: ignore[tenant-scope]
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
