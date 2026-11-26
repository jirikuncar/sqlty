---
order: 319
group: "names"
severity: "warn"
fixable: false
summary: "Row-level security does not scope a tenant table for every role and command"
origin: "Migration"
---

# tenant-policy

Default severity: **warn**

## What it does

Reports, in a PostgreSQL schema file, a tenant table with row-level security that the
database does not scope to one tenant: its owner skips the policies, or a policy does
not compare the tenant key with the session for some command. `sqlty check` reports it
on the statement to change: the `ALTER TABLE` that enabled row-level security, or the
`CREATE POLICY`.

A tenant table is one with a tenant key, from
[`tenancy`](../configuration.md#tenancy) or from a policy that compares a column with
a session value, as in [`tenant-scope`](tenant-scope.md#row-level-security).

```sql schema
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, name TEXT NOT NULL);
-- error: [tenant-policy] "`projects` has row-level security without `FORCE ROW LEVEL SECURITY`, so its owner sees every tenant"
ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON projects USING (org_id = current_setting('app.org_id')::int);

CREATE TABLE invoices (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, total NUMERIC NOT NULL);
ALTER TABLE invoices ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY;
-- error: [tenant-policy] "policy `tenant` on `invoices` does not check `org_id` on INSERT, UPDATE; compare it to the session in WITH CHECK"
CREATE POLICY tenant ON invoices USING (org_id = current_setting('app.org_id')::int) WITH CHECK (true);
```

Both tables are scoped once `projects` forces row-level security and the policy of
`invoices` checks the rows it lets in:

```sql schema
-- error:
CREATE TABLE projects (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, name TEXT NOT NULL);
ALTER TABLE projects ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON projects USING (org_id = current_setting('app.org_id')::int);

CREATE TABLE invoices (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, total NUMERIC NOT NULL);
ALTER TABLE invoices ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant ON invoices USING (org_id = current_setting('app.org_id')::int);
```

A policy for one command is judged for that command alone, and a restrictive policy
that pins the tenant key covers the permissive ones:

```sql schema
CREATE TABLE notes (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, body TEXT);
ALTER TABLE notes ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY;
CREATE POLICY mine ON notes FOR SELECT USING (org_id = current_setting('app.org_id')::int);
-- error: [tenant-policy] "policy `cleanup` on `notes` does not filter DELETE by `org_id`; compare it to the session in USING"
CREATE POLICY cleanup ON notes FOR DELETE USING (true);
CREATE POLICY writes ON notes FOR INSERT WITH CHECK (org_id = current_setting('app.org_id')::int);
CREATE POLICY edits ON notes FOR UPDATE USING (org_id = current_setting('app.org_id')::int);

CREATE TABLE tags (id SERIAL PRIMARY KEY, org_id INTEGER NOT NULL, name TEXT);
ALTER TABLE tags ENABLE ROW LEVEL SECURITY, FORCE ROW LEVEL SECURITY;
CREATE POLICY open ON tags USING (true);
CREATE POLICY tenant ON tags AS RESTRICTIVE USING (org_id = current_setting('app.org_id')::int);
```

Until then, [`tenant-scope`](tenant-scope.md) judges the table's queries. Open this
example in the playground to see both, the policy in `schema.sql`:

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

## Why is this bad?

[`tenant-scope`](tenant-scope.md) leaves the queries of a table to the database only
when its policies hold for everyone. While they don't, it reports each query that does
not filter by the tenant key, and this rule says what to change in the schema so the
database enforces it:

- The owner of a table skips its policies unless the table has
  `FORCE ROW LEVEL SECURITY`, and an application often connects as the owner.
- PostgreSQL combines permissive policies with `OR`, so one that does not compare the
  tenant key with the session, such as `USING (true)`, lets every tenant's rows through.
- `WITH CHECK (true)` lets an `INSERT` or `UPDATE` write a row of another tenant.
  Without `WITH CHECK`, a policy checks new rows with `USING`.

Superusers and `BYPASSRLS` roles skip every policy, `FORCE` or not: connect as a role
that has neither. See
[Row Security Policies](https://www.postgresql.org/docs/17/ddl-rowsecurity.html).

## How to fix

Add `ALTER TABLE ... FORCE ROW LEVEL SECURITY`, and compare the tenant key with the
session in each permissive policy: in `USING` for the rows it reads, updates or
deletes, and in `WITH CHECK` for the rows it writes. Or add a restrictive policy that
does.

## Configuration

A warning by default: the queries are still judged by `tenant-scope` meanwhile. Turn it
off:

```toml
[tool.sqlty.rules]
tenant-policy = "ignore"
```

`-- sqlty: ignore[tenant-policy]` on the first line of a statement, or alone on its
line above the statement, drops it.

## Limits

Only the PostgreSQL dialect reads policies. Policies for some roles only (`TO app`)
count as permissive policies for every role when they let rows through, and as nothing
when they are restrictive, since sqlty does not know which role the application uses.
`sqlty check` reads the schema files from disk: an editor shows a schema file's
diagnostics once it is saved.
