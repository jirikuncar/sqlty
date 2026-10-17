# Migration

sqlty only acts on strings that start with `--sql` (or `-- sql`), so you can adopt it one
module, or one query, at a time. Unconverted code keeps working.

## Before and after

A typical untyped query:

```python
def get_user(conn, user_id):
    row = conn.execute("SELECT id, name, created_at FROM users WHERE id = %s", (user_id,)).fetchone()
    return row  # tuple[Any, ...] | None: nothing says what the columns are
```

The same query after migration, with a [schema](../guide/schema-inference.md)
and a [row type](../guide/row-types.md):

```python
from dataclasses import dataclass
from datetime import datetime

from sqlty import SQL
from sqlty.drivers.psycopg import execute

@dataclass
class User:
    id: int
    name: str
    created_at: datetime

GET_USER = SQL[User]("--sql\nSELECT id, name, created_at FROM users WHERE id = %s")

def get_user(conn, user_id: int) -> User | None:
    return execute(conn, GET_USER, (user_id,)).fetchone()
```

`sqlty check` now fails if `users` changes shape in the schema and `User`
does not follow.

## Strategy

### 1. Configure a schema first

A schema saves you from adding casts to every query. Dump it from the
database and commit it:

```bash
pg_dump --schema-only app > schema.sql      # PostgreSQL
sqlite3 app.db .schema > schema.sql         # SQLite
mysqldump --no-data app > schema.sql        # MySQL
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE public.users (
    id integer NOT NULL,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now()
);
```

Statements other than `CREATE TABLE` in the dump are ignored, and
schema-qualified tables match unqualified queries:

```python
query = "--sql\nSELECT id, name, created_at FROM users WHERE id = %s"
# rewritten: query = SQL[tuple[int, str, datetime.datetime | None]]("--sql\nSELECT id, name, created_at FROM users WHERE id = %s")
```

Without a schema, an uncast column has no known type, so the query is left
as it is (see the next step).

### 2. Mark untyped queries

Turn on the sentinel so queries sqlty cannot type are visible instead of
silently left as strings:

```toml
[tool.sqlty]
untyped = "sentinel"
```

`sqlty check` then reports each one as
[`untyped-query`](../reference/rules.md#untyped-query) until you add casts or
schema entries. See [Untyped queries](../guide/untyped.md).

### 3. Prefix queries, one module at a time

Add `--sql\n` to the start of each query in a module, then rewrite it:

```bash
sqlty rewrite app/users.py
```

`sqlty check` lists prefixed strings that are still unwrapped as
[`stale-rewrite`](../reference/rules.md#stale-rewrite), so you can see what is
left:

```
app/orders.py:12: [stale-rewrite] Source has --sql strings that aren't wrapped in SQL[T]
```

Statements without rows (`INSERT`, `UPDATE`, `DELETE` without `RETURNING`,
DDL) are never wrapped, and so never reported.

### 4. Name important rows

Replace `tuple[...]` with a row type where a query feeds domain code. Start
with the queries whose rows are passed around the most.

### 5. Enforce

Run `sqlty rewrite --check` and `sqlty check` in [CI](ci.md), and promote
[`stale-type`](../reference/rules.md#stale-type) to an error once the codebase
is clean.

## Editing typed queries

Once a module has row types, edit its queries in place and let `sqlty check`
report a stale `T`; do not strip it. See
[Editing queries](../guide/writing-queries.md#editing-queries).

## Pitfalls

**`--sql` followed by a space.** `"--sql SELECT ..."` is a SQL comment: the
query is empty to sqlty and the string is never wrapped. Use a newline.
`sqlty check` reports it as
[`inline-sql-comment`](../reference/rules.md#inline-sql-comment).

```python
wrong = "--sql SELECT id::INTEGER FROM users"
right = "--sql\nSELECT id::INTEGER FROM users"
# rewritten: wrong = "--sql SELECT id::INTEGER FROM users"
# rewritten: right = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM users")
```

**`strip` removes hand-written row types.** It turns `SQL[User](...)` back
into a plain string; see
[Editing queries](../guide/writing-queries.md#editing-queries).

**Interpolated queries.** f-strings and t-strings cannot be rewritten. Pass
values as parameters (`%s`) instead.

**String operations drop the runtime type.** `query + " LIMIT 1"` is a plain
`str`, so [`row_form`](../guide/row-types.md#the-row-type-at-runtime) and
`execute` no longer know `T`. Put the complete query in the literal.

**Nullable columns.** A column that can be `NULL` infers `T | None`, so a row type
field must be optional too ([`nullable-mismatch`](../reference/rules.md#nullable-mismatch)).
Wrappers written by an earlier release are `stale-type`: `sqlty check --fix` updates
them. See [Nullability](../guide/schema-inference.md#nullability).

## Schema changes in CI

Once your queries are typed, a pull request that changes the schema can break
the code still running during the deploy. `sqlty breaking` checks the queries of
the base branch against the new schema, and the new queries against the old
schema:

```yaml
# .github/workflows/sqlty-breaking.yml
name: sqlty breaking
on: [pull_request]
jobs:
  breaking:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0  # the base branch must be in the clone
      - uses: astral-sh/setup-uv@v7
      - run: uv sync
      - run: uv run sqlty breaking --against origin/${{ github.base_ref }}
```

Use `--direction backward` when migrations always run before the deploy, and
`--direction forward` when the code always ships first. See
[Breaking schema changes](../guide/breaking-changes.md).

Related issue:
[#19 Create migration tool for existing codebases](https://github.com/jirikuncar/sqlty/issues/19).
