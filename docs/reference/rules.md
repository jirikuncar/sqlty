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

| Rule | Group | Default | Fixable | Reports |
|------|-------|---------|---------|---------|
| [`column-count-mismatch`](#column-count-mismatch) | Row type | error | no | Row type has a different number of fields than the query has columns |
| [`column-type-mismatch`](#column-type-mismatch) | Row type | error | no | Row type field can't hold its column's values |
| [`column-name-mismatch`](#column-name-mismatch) | Row type | warn | no | `NamedTuple` field name differs from its column name |
| [`typeddict-key-mismatch`](#typeddict-key-mismatch) | Row type | error | no | Columns don't fit the fields of a TypedDict, dataclass or Pydantic model |
| [`typeddict-inheritance`](#typeddict-inheritance) | Row type | error | no | TypedDict subclass breaks a closed or `extra_items` base (PEP 728) |
| [`select-star-no-schema`](#select-star-no-schema) | Query | error | no | `SELECT *` with no schema to expand it |
| [`select-star`](#select-star) | Query | warn | yes | `SELECT *` |
| [`inline-sql-comment`](#inline-sql-comment) | Query | warn | yes | `"--sql SELECT ..."`: the query is a SQL comment |
| [`mysql-sql-marker`](#mysql-sql-marker) | Query | warn | yes | `--sql` marker with `dialect = "mysql"`, which MySQL rejects |
| [`unparsed-query`](#unparsed-query) | Query | ignore | no | Query that does not parse, so sqlty cannot check it |
| [`stale-rewrite`](#stale-rewrite) | Rewrite | warn | yes | `--sql` string not wrapped in `SQL[T](...)` |
| [`stale-type`](#stale-type) | Rewrite | warn | yes | `SQL[tuple[...]]` neither matches nor refines the inferred row type |
| [`fstring-skipped`](#fstring-skipped) | Rewrite | warn | no | f-string with `--sql` cannot be rewritten |
| [`tstring-skipped`](#tstring-skipped) | Rewrite | warn | no | t-string with `--sql` cannot be rewritten |
| [`unchecked-query`](#unchecked-query) | Rewrite | warn | no | `--sql` query that is not plain string literals |
| [`untyped-query`](#untyped-query) | Rewrite | warn | yes | `SQL[UNTYPED]` query |
| [`unused-ignore`](#unused-ignore) | Suppression | ignore | no | `# sqlty: ignore` comment that suppresses nothing |

`sqlty check --fix` applies the fixable rules; `sqlty rule NAME` prints a
rule's page.

## Inline suppression

A comment on the line a diagnostic is reported on drops it:

- `# sqlty: ignore` drops every diagnostic on that line.
- `# sqlty: ignore[rule-a, rule-b]` drops only the listed rules. A name that
  matches no diagnostic on the line drops nothing, so the diagnostic stays.

The comment is read with Python's tokenizer, so a `#` inside a string does
not start one. For a statement that spans lines, put the comment on the line
the diagnostic names. `sqlty check --fix` keeps the comment and does not apply
a fix for a rule the comment suppresses.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error:
q = SQL[User]("--sql\nSELECT id::INTEGER FROM users")  # sqlty: ignore[column-type-mismatch]
```

Enable [`unused-ignore`](#unused-ignore) to report comments that suppress
nothing, so accepted cases do not outlive their reason.

The examples below show what `sqlty check` reports for each block. `# error:`
lines sit above the line they report on, warnings included; a bare `# error:`
means nothing is reported.

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

# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Fix: add or remove fields, or change the query.

### column-type-mismatch

Default: error. A field can't hold the values of its column's inferred type.
Also reported for extra columns that don't fit a TypedDict's `extra_items`
type. A field typed `Any`, a union containing `Any` or the column type, or
`float` for an `int` column is accepted, as is a `NewType` wherever its
supertype is and a parametrised generic (`list[str]`, `dict[str, int]`) for a
column that infers `list` or `dict` (element types are not compared; a JSON
column infers `Any`, which any field accepts); a narrower field, such as `bool` for
an `int` column, is not; see `sqlty rule column-type-mismatch`.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str
    name: str

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Fix: change the field type or the cast.

### column-name-mismatch

Default: warn. A `NamedTuple` field name differs from its column name or
alias. `NamedTuple` fields are matched by position, so the row would still
be built, with misleading names. TypedDicts, dataclasses and Pydantic models
are matched by name instead; see `typeddict-key-mismatch`.

```python
from typing import NamedTuple
from sqlty import SQL

class User(NamedTuple):
    user_id: int
    name: str

# error: [column-name-mismatch] "User.user_id doesn't match query column 'id'"
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Fix: rename the field or alias the column (`id AS user_id`).

### typeddict-key-mismatch

Default: error. TypedDicts, dataclasses and Pydantic models are matched to
columns by name. The query has a column with no field (a Pydantic model
ignores it, and `extra_items` admits it), or lacks a required field. A
field with a default, or a `NotRequired` key, may be missing.

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict, closed=True):
    id: int
    name: str

# error: [typeddict-key-mismatch] "User has no key for query column 'email'"
# error: [typeddict-key-mismatch] "User.name is required but missing from query"
q = SQL[User]("--sql\nSELECT id::INTEGER, email::TEXT FROM users")
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

# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Base):
    name: str

q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Fix: see [TypedDict inheritance](../guide/row-types.md#typeddict-inheritance)
for what a subclass may do.

## Suppression rules

### unused-ignore

Default: ignore. A `# sqlty: ignore` comment that suppresses no diagnostic on
its line, or a rule listed in one that suppresses nothing. Off by default;
enable it with `unused-ignore = "warn"`. It reports, for example:

```text
'# sqlty: ignore[select-star]' suppresses no diagnostic
```

Fix: remove the comment, or correct the rule name.

## Query rules

### select-star-no-schema

Default: error. `SELECT *` without a schema: sqlty cannot know the columns.

```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
q = SQL[tuple[int]]("--sql\nSELECT * FROM users")
```

Fix: list the columns, or configure a
[schema](../guide/schema-inference.md). The annotation is not compared with an inferred type, and `check --fix` does not change it.

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

```python
from sqlty import SQL

# error: [select-star] "SELECT * used — expand to explicit column list (joins need a manual edit)"
q = SQL[tuple[int, str]]("--sql\nSELECT * FROM users")
```

Fix: `sqlty check --fix` replaces `*` with the cast column list from the
schema, unless the string holds escapes or quotes, the query is split over
several concatenated strings, or it reads from more than one table (`JOIN` or
`FROM a, b`), because the expansion drops table qualifiers. A `*` the schema
cannot resolve (a `USING` or `NATURAL` join) gets no inferred type, so its
annotation is kept. A `*` over a subquery or CTE is typed from that query's
columns but not expanded by `--fix`.

### inline-sql-comment

Default: warn. `--sql` is followed by a space instead of a newline, so the
whole query is a SQL comment and the string is never wrapped.

```python
# error: [inline-sql-comment] ""--sql SELECT ..." treats the query as a SQL comment; use "--sql\nSELECT ..." or run sqlty rewrite"
q = "--sql SELECT id::INTEGER FROM users"
```

Only a statement after the space is reported: `SELECT` with a column,
`WITH ... AS (`, `INSERT INTO`, `UPDATE t SET`, `DELETE FROM`, `VALUES (`,
DDL such as `CREATE TABLE`, `TRUNCATE`, `MERGE INTO` or `EXPLAIN`. Text such as
`"--sql flag enables it"` is not.

Fix: `sqlty check --fix` replaces the space after `--sql` with `\n` in plain
string literals (not raw, f- or t-strings) that this rule reports;
`sqlty rewrite` never changes string contents.

### mysql-sql-marker

Default: warn. With `dialect = "mysql"`, a plain string literal opens with the
`--sql` marker. MySQL and MariaDB read `--` as a comment only when whitespace follows
it, so the server rejects the query with a syntax error (1064). `-- sql` is a
comment in every dialect and sqlty reads it as the marker too.

```toml
[tool.sqlty]
dialect = "mysql"
```

```python
from sqlty import SQL

# error: [mysql-sql-marker] "MySQL reads "--sql" as an expression, not a comment; use "-- sql""
q = SQL[tuple[int]]("--sql\nSELECT CAST(id AS SIGNED) FROM users")
```

Fix: `sqlty check --fix` replaces `--sql` with `-- sql` in plain string
literals, wrapped or not.

### unparsed-query

Default: ignore. A query that does not parse in the configured dialect: a
`--sql` string, the argument of `SQL[T](...)`, or a string passed to an
`Annotated[LiteralString, SQL]` parameter. The message quotes sqlparser's
error. Enable it with `unparsed-query = "warn"`:

```toml
[tool.sqlty.rules]
unparsed-query = "warn"
```

```python
from sqlty import SQL

# error: [unparsed-query] "query does not parse: Expected: ), found: foo"
q = SQL[tuple[str]]("--sql\nSELECT xmlelement(name foo)")
```

Fix: rewrite the query in a form the parser reads. `sqlty check --statistics`
counts such queries as unparsed whether or not the rule is enabled.

## Rewrite rules

### stale-rewrite

Default: warn. A `--sql` string that `rewrite` would wrap is not wrapped in
`SQL[T](...)`. The marker may follow whitespace or a line break, as in a
triple-quoted string that opens on its own line; implicitly concatenated
strings are one query.

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
q = "--sql\nSELECT id::INTEGER FROM users"
```

Fix: `sqlty rewrite` or `sqlty check --fix`. Only strings `rewrite` would wrap
are reported: statements without rows, uninferable queries in `skip` mode,
the `"--sql SELECT"` form, and strings that are not plain literals
(f-, t-, raw or bytes strings, alone or in a concatenation) are not.

### stale-type

Default: warn. The `T` in `SQL[tuple[...]]` differs from the type sqlty infers
now, for example after the query or the schema changed. An item that only
parametrises an inferred bare generic (`list[str]` for `list`) is not stale.
Named row types are covered by the row type rules instead.

```python
from sqlty import SQL

# error: [stale-type] "SQL[tuple[int]] should be SQL[tuple[int, str]]"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

Fix: `sqlty check --fix` writes the inferred `T`, keeping parametrised
generics; see also
[Editing queries](../guide/writing-queries.md#editing-queries).

### fstring-skipped

Default: warn. An f-string starting with `--sql` is skipped by the rewriter,
because its content is only known at runtime:

```python
# error: [fstring-skipped] "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
q = f"--sql\nSELECT {column}::INTEGER FROM users"
```

Fix: use query parameters instead of interpolation.

### tstring-skipped

Default: warn. Same as `fstring-skipped`, for t-strings
([PEP 750](https://peps.python.org/pep-0750/)).

```python
# error: [tstring-skipped] "t-string with --sql prefix skipped (t-strings cannot be statically rewritten)"
q = t"--sql\nSELECT {column}::INTEGER FROM users"
```

A concatenation that holds an f- or t-string anywhere is reported the same way.

### unchecked-query

Default: warn. A `--sql` query that is not plain string literals: a raw or
bytes string (alone or in a concatenation), or a `SQL[T](...)` argument with
anything but string literals. sqlty neither checks nor rewrites it.

```python
from sqlty import SQL

# error: [unchecked-query] "--sql query is not plain string literals, so it is not checked or rewritten"
q = SQL[tuple[int]]("--sql\nSELECT id::INTEGER FROM " + table)
```

Fix: write the query as one plain literal, or implicitly concatenated ones.
`SQL[Any](...)`, or `Any` as a helper's row type, opts out explicitly.

### untyped-query

Default: warn. A query is wrapped as `SQL[UNTYPED](...)`
([Untyped queries](../guide/untyped.md)). Once casts or a schema make the type
known, the message suggests it.

```python
# error: [untyped-query] "SQL[UNTYPED] has no inferable row type; add casts (e.g. id::INTEGER) or a schema"
q = SQL[UNTYPED]("--sql\nVALUES (1, 2)")
```

Fix: add casts or schema entries; `sqlty check --fix` then writes the row
type.
