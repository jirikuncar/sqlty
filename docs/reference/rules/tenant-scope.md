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
key from one. It runs only when [`tenancy`](../configuration.md#tenancy) names the
tenant table.

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

The tenant keys are the primary key of the tenant table and every column of a
foreign key to it, less `exclude`, plus `columns`. A table with a tenant key is a
tenant table. A tenant table's key is pinned when:

- `WHERE`, or the `ON` of an inner join, compares it with a parameter: `= %s`,
  `IN (%s, %s)` or `= ANY(%s)`, cast or not;
- it equals a pinned tenant key of another table, through `=` or `USING`;
- it is a tenant key of an enclosing query's row, which is judged on its own.

A `LEFT JOIN`'s `ON` pins the joined table only, since every row of the left side is
kept; a `RIGHT JOIN`'s pins the others, and a `FULL JOIN`'s none. A table with two
tenant keys, such as `transfers (from_org, to_org)`, is pinned by either.

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

A table with `ENABLE ROW LEVEL SECURITY` is not judged: the database scopes it. An
`INSERT ... SELECT` is judged as the query it runs, and the tenant table itself may be
inserted into freely. A CTE or derived table is judged by its own body: a pin in the query that reads it
does not reach it, so pin the key inside the CTE. A view is not judged.
