# Rules

`sqlty check` reports diagnostics as `file:line: [rule] message`. Each rule
has a default severity, changed per rule in `[tool.sqlty.rules]`:

```toml
[tool.sqlty.rules]
column-name-mismatch = "error"
stale-type = "error"
```

`error` makes `sqlty check` exit with 1, `warn` prints without failing, and
`ignore` turns the rule off.

| Rule | Default | Fixable | Reports |
|------|---------|---------|---------|
| [`column-count-mismatch`](#column-count-mismatch) | error | no | Row type has a different number of fields than the query has columns |
| [`column-type-mismatch`](#column-type-mismatch) | error | no | Row type field has a different type than its column |
| [`column-name-mismatch`](#column-name-mismatch) | warn | no | Row type field name differs from its column name |
| [`typeddict-key-mismatch`](#typeddict-key-mismatch) | error | no | Columns don't fit the keys of a closed or `extra_items` TypedDict |
| [`typeddict-inheritance`](#typeddict-inheritance) | error | no | TypedDict subclass breaks a closed or `extra_items` base (PEP 728) |
| [`select-star`](#select-star) | warn | yes | `SELECT *` |
| [`select-star-no-schema`](#select-star-no-schema) | error | no | `SELECT *` with no schema to expand it |
| [`stale-rewrite`](#stale-rewrite) | warn | yes | `--sql` string not wrapped in `SQL[T](...)` |
| [`stale-type`](#stale-type) | warn | yes | `SQL[tuple[...]]` differs from the inferred row type |
| [`inline-sql-comment`](#inline-sql-comment) | warn | yes | `"--sql SELECT ..."`: the query is a SQL comment |
| [`fstring-skipped`](#fstring-skipped) | warn | no | f-string with `--sql` cannot be rewritten |
| [`tstring-skipped`](#tstring-skipped) | warn | no | t-string with `--sql` cannot be rewritten |
| [`untyped-query`](#untyped-query) | warn | no | `SQL[UNTYPED]` query |

"Fixable" means the fix is mechanical, as described under each rule.
`sqlty check --fix` does not apply fixes yet.

The examples below show what `sqlty check` reports for each block; an empty
`# error:` means nothing.

## Row type rules

These rules compare a [row type](../guide/row-types.md) (`TypedDict`,
`NamedTuple`, dataclass, Pydantic model) defined in the same file with the
query's columns.

### column-count-mismatch

Default: error. The row type has more or fewer fields than the query
returns columns. Checked before types and names.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
```

Fix: add or remove fields, or change the query.

### column-type-mismatch

Default: error. A field's type differs from its column's inferred type. Also
reported for extra columns that don't match a TypedDict's `extra_items`
type.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
```

Fix: change the field type or the cast.

### column-name-mismatch

Default: warn. A field name differs from its column name or alias. Fields
are matched by position, so the row would still be built, with misleading
names.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    user_id: int
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [column-name-mismatch] "User.user_id doesn't match query column 'id'"
```

Fix: rename the field or alias the column (`id AS user_id`).

### typeddict-key-mismatch

Default: error. A closed TypedDict, or one with `extra_items`, is matched by
key. The query has a column with no key, or lacks a required key.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
```

Fix: add the key (as `NotRequired` if it may be missing), allow
`extra_items`, or change the query. See
[Closed TypedDicts](../guide/row-types.md#closed-typeddicts).

### typeddict-inheritance

Default: error. A TypedDict subclass adds keys to a closed base, reopens it,
or changes a base's `extra_items` in a way PEP 728 forbids. Reported on the
subclass's `class` line.

```python
from typing import TypedDict
from sqlty import SQL

class Base(TypedDict, closed=True):
    id: int

class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
```

Fix: see [TypedDict inheritance](../guide/row-types.md#typeddict-inheritance)
for what a subclass may do.

## Query rules

### select-star-no-schema

Default: error. `SELECT *` without a schema: sqlty cannot know the columns.

```python
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
# error: [select-star-no-schema] "SELECT * used but no schema available"
# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[str]]"
```

Fix: list the columns, or configure a
[schema](../guide/schema-inference.md).

### select-star

Default: warn. `SELECT *` with a schema. The row type follows the table,
so any change to the table changes it.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```sql
SELECT * FROM users
```

```python
# error: [select-star] "SELECT * used — expand to explicit column list"
```

Fix: replace `*` with the column list.

### inline-sql-comment

Default: warn. `--sql` is followed by a space instead of a newline, so the
whole query is a SQL comment and the string is never wrapped.

```python
q = "--sql SELECT id::INTEGER FROM users"
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
```

Such a string is also reported as `stale-rewrite`. Fix: replace the space
after `--sql` with `\n`; `sqlty rewrite` does not change string contents.

## Rewrite rules

### stale-rewrite

Default: warn. A string starting with `--sql` is not wrapped in
`SQL[T](...)`.

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
```

Fix: run `sqlty rewrite`. The rule also fires for strings `rewrite` leaves
alone: statements without rows, uninferable queries in `skip` mode, f- and
t-strings, and the `"--sql SELECT"` form. Use `sqlty rewrite --check` to
fail only on strings that would change (see [CI](../ci.md)).

### stale-type

Default: warn. The `T` in `SQL[tuple[...]]` differs from the type sqlty infers
now, for example after the query or the schema changed. Named row types are
covered by the row type rules instead.

```python
from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
```

Fix: `sqlty strip` and `sqlty rewrite` the file, or edit `T`.

### fstring-skipped

Default: warn. An f-string starting with `--sql` is skipped by the rewriter,
because its content is only known at runtime. The rewriter API returns it
as a warning:

```python
q = f"--sql\nSELECT {column}::INTEGER FROM users"
# warning: line 1: f-string with --sql prefix skipped (f-strings cannot be statically rewritten) [fstring-skipped]
```

The CLI does not print this warning; `sqlty check` reports the string as
`stale-rewrite`. Fix: use query parameters instead of interpolation.

### tstring-skipped

Default: warn. Same as `fstring-skipped`, for t-strings
([PEP 750](https://peps.python.org/pep-0750/)).

```python
q = t"--sql\nSELECT {column}::INTEGER FROM users"
# warning: line 1: t-string with --sql prefix skipped (t-strings cannot be statically rewritten) [tstring-skipped]
```

### untyped-query

Default: warn. A query is wrapped as `SQL[UNTYPED](...)`
([Untyped queries](../guide/untyped.md)). Once casts or a schema make the type
known, the message suggests it.

```python
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
```

Fix: add casts or schema entries, then strip and rewrite.
