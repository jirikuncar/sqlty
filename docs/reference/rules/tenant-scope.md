---
order: 318
group: "names"
severity: "error"
fixable: false
summary: "Query on a tenant table does not filter by the tenant key"
origin: "SQL"
---

# tenant-scope

Default severity: **error**

## What it does

Reports a statement that reads, updates or deletes rows of a tenant table without
pinning its tenant key to a parameter, and an `INSERT` that does not set the tenant
key from one. It runs when [`tenancy`](../configuration.md#tenancy) names the
tenant table, or when a PostgreSQL row-level security policy compares a column to the
session's tenant.

```toml
[tool.sqlty]
schema = "schema.sql"

[tool.sqlty.tenancy]
table = "orgs"
exclude = ["audit_log.org_id"]
columns = ["events.org"]
```

`schema.sql`:

```sql
CREATE TABLE orgs (id SERIAL PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES orgs, name TEXT NOT NULL);
CREATE TABLE invoices (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES orgs, project_id INTEGER NOT NULL REFERENCES projects);
CREATE TABLE audit_log (id SERIAL PRIMARY KEY, org_id INTEGER REFERENCES orgs, message TEXT NOT NULL);
CREATE TABLE events (id BIGSERIAL PRIMARY KEY, org INTEGER NOT NULL, payload JSONB);
```

```python
from sqlty import SQL

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
names = SQL[tuple[str]]("--sql\nSELECT name FROM projects")

# error: [tenant-scope] "`invoices` is read without `invoices.org_id = <parameter>`"
leaky = SQL[tuple[int]]("--sql\nSELECT i.id FROM invoices i JOIN projects p ON p.id = i.project_id WHERE p.org_id = %s")

# error: [tenant-scope] "`events` is deleted from without `events.org = <parameter>`"
purge = SQL("--sql\nDELETE FROM events WHERE id < %s")

# error: [tenant-scope] "INSERT into `projects` does not set `org_id` from a parameter"
create = SQL("--sql\nINSERT INTO projects (org_id, name) VALUES (1, %s)")
```

Each of these is scoped to one tenant:

```python
from sqlty import SQL

# error:
mine = SQL[tuple[str]]("--sql\nSELECT name FROM projects WHERE org_id = %s")

# error:
joined = SQL[tuple[int]]("--sql\nSELECT i.id FROM invoices i JOIN projects p ON p.org_id = i.org_id AND p.id = i.project_id WHERE p.org_id = %s")

# error:
audit = SQL[tuple[str]]("--sql\nSELECT message FROM audit_log ORDER BY id DESC LIMIT 100")

# error:
create = SQL("--sql\nINSERT INTO projects (org_id, name) VALUES (%s, %s)")
```

A statement that spans tenants on purpose, such as an admin report or a cleanup job,
says so with an ignore comment:

```python
from sqlty import SQL

# error:
counts = SQL[tuple[int, int]]("--sql\nSELECT org_id, count(*)::INT FROM projects GROUP BY org_id")  # sqlty: ignore[tenant-scope]
```

The tenant keys are the primary key of the tenant table and every foreign key to
it, less those with a column in `exclude`, plus each column of `columns`. A table
with a tenant key is a tenant table. It is scoped when every column of one of its
tenant keys is pinned, and a column is pinned when:

- `WHERE`, or the `ON` of an inner join, compares it with a parameter: `= %s`,
  `IN (%s, %s)` or `= ANY(%s)`, cast or not;
- it equals a pinned tenant key of another table, through `=` or `USING`;
- it is a tenant key of an enclosing query's row, which is judged on its own.

A CTE or derived table that returns a tenant key leaves it to the query that reads it,
which pins it as it pins a table's:

```python
from sqlty import SQL

# error:
recent = SQL[tuple[str]]("--sql\nWITH p AS (SELECT name, org_id FROM projects) SELECT name FROM p WHERE org_id = %s")

# error: [tenant-scope] "`projects` is read without `projects.org_id = <parameter>`"
leaked = SQL[tuple[str]]("--sql\nWITH p AS (SELECT name, org_id FROM projects) SELECT name FROM p")
```

A CTE whose rows depend on other tenants' rows is judged by its body: one with a window,
`DISTINCT ON`, `LIMIT`, or a grouping that is not by the tenant key.

A `LEFT JOIN`'s `ON` pins the joined table only, since every row of the left side is
kept; a `RIGHT JOIN`'s pins the others, and a `FULL JOIN`'s none. A table with two
tenant keys, such as `transfers (from_org, to_org)`, is pinned by either.

A write must not move a row to another tenant either: an `INSERT` sets each tenant
key column from a parameter (or, in `INSERT ... SELECT`, from a column of the query,
which is judged itself), and an `UPDATE` or `ON CONFLICT DO UPDATE` that sets one
sets it from a parameter or `EXCLUDED`.

## Why is this bad?

A query that forgets the tenant filter returns, changes or deletes the rows of every
tenant. Nothing fails: the query is valid and the rows have the right types, so the
leak shows up in production. A foreign key to the tenant table on one column does not
help when the join goes through another: `invoices.project_id REFERENCES projects (id)`
accepts an invoice of one tenant on a project of another, and `leaky` above returns it.

## How to fix

Compare the tenant key of every tenant table with the tenant parameter, or with a
pinned tenant key of a table joined to it: `DELETE FROM events WHERE org = %s AND id < %s`.

## Configuration

`[tool.sqlty.tenancy]` takes:

| Key | Value |
|-----|-------|
| `table` | The tenant table, whose primary key is the tenant id |
| `exclude` | `table.column` tenant keys of data shared across tenants, such as an audit log |
| `columns` | `table.column` tenant ids that have no foreign key, such as those of a partitioned or external table |
| `session-functions` | Functions a row-level security policy reads the session's tenant from, such as `auth.uid` |

## Row-level security

In PostgreSQL, a policy that compares a column to a session value makes the column a
tenant key, with or without `[tool.sqlty.tenancy]`. Session values are
`current_setting(...)`, `current_user`, `session_user` and the functions of
`session-functions`, cast or not, or alone in a subquery: `(SELECT auth.uid())`.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE notes (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, body TEXT);
ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON notes USING (org_id = current_setting('app.org_id')::int);
```

```python
from sqlty import SQL

# error: [tenant-scope] "`notes` is read without `notes.org_id = <parameter>`"
notes = SQL[tuple[str | None]]("--sql\nSELECT body FROM notes")
```

The query is reported because the database does not enforce the policy for every role:
superusers, `BYPASSRLS` roles and the owner of a table without
`FORCE ROW LEVEL SECURITY` skip it. A table is left to the database only when it has
`ENABLE` and `FORCE ROW LEVEL SECURITY`, and for each of `SELECT`, `INSERT`, `UPDATE`
and `DELETE` a restrictive policy for every role pins a tenant key, or each permissive
policy does. A row an `INSERT` or `UPDATE` writes must pass `WITH CHECK`, or `USING`
when the policy has none, so `WITH CHECK (true)` lets an `INSERT` write a row of
another tenant and the table is still judged.

```sql
ALTER TABLE notes FORCE ROW LEVEL SECURITY;
```

With it, `SELECT body FROM notes` is not reported. Superusers and `BYPASSRLS` roles
still skip the policy: connect as a role that has neither.

## Limits

An `INSERT ... SELECT` is judged as the query it runs, and the tenant table itself may be
inserted into freely. A recursive CTE is judged by its own body: a pin in the query that
reads it does not reach it, so pin the key inside the CTE. So is a CTE or derived table a
`MERGE` reads. Views and `MERGE` itself are not judged.
The rule runs for PostgreSQL, MySQL and SQLite, the dialects whose names sqlty checks.
