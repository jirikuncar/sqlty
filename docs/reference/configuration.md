---
description: "Reference for every sqlty configuration key in sqlty.toml, pyproject.toml or Cargo.toml: schema, dialect, queries, rules and file selection."
---

# Configuration

sqlty reads `sqlty.toml`, `[tool.sqlty]` from `pyproject.toml`, or `[package.metadata.sqlty]`
or `[workspace.metadata.sqlty]` from `Cargo.toml`. The CLI uses, for each file, the nearest
of them above it, unless
[`--config-file` or `--isolated`](cli.md#configuration-flags) picks one file or none; the magic codec
searches from the current directory. Command-line flags override the file. An unknown key, rule or
severity is an error (exit 2) that names the file and suggests the closest match.

In every case a `pyproject.toml` without `[tool.sqlty]`, or a `Cargo.toml` without
either Cargo table, is skipped, so a package's packaging-only file in a monorepo
does not hide the root config. The closest file wins; settings are not merged with files in
parent directories, unless it names one with [`extend`](#extend). Command-line flags (`--dialect`,
`--schema`, `--untyped`, `--exclude`, `--extend-exclude`, `--error`/`--warn`/`--ignore`)
and `-c KEY=VALUE` assignments override the file for every path.

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
import-style = "from sqlty import SQL"
untyped = "skip"

[tool.sqlty.rules]
select-star = "ignore"
stale-type = "error"

[[tool.sqlty.functions]]
import-path = "db.sql"
query-param = "query"
```

## `sqlty.toml`

`sqlty.toml` holds the same keys at the top level, without the `tool.sqlty` prefix.

`sqlty.toml`:

```toml
dialect = "postgres"
schema = "schema.sql"

[rules]
select-star = "ignore"

[[functions]]
import-path = "db.sql"
query-param = "query"
```

A project without a `pyproject.toml` needs no Python packaging file to hold
its settings. In a directory with both, `sqlty.toml` wins and `pyproject.toml` is
not read, as in ruff. Relative paths start from the directory of the file
that holds them, whichever it is.

## `Cargo.toml`

A Rust crate keeps the same keys under `[package.metadata.sqlty]`, where Cargo
allows tool settings and ignores them.

`Cargo.toml`:

```toml
[package]
name = "app"
version = "0.1.0"

[package.metadata.sqlty]
dialect = "postgres"
schema = "schema.sql"

[package.metadata.sqlty.rules]
select-star = "ignore"
```

A Cargo workspace keeps them under `[workspace.metadata.sqlty]` of its root
`Cargo.toml`. A member crate without its own `[package.metadata.sqlty]` uses the
workspace table, since the upward search skips the member's `Cargo.toml` and reaches
the root. A root `Cargo.toml` with both tables reads `[package.metadata.sqlty]`.

`Cargo.toml`:

```toml
[workspace]
members = ["app", "worker"]

[workspace.metadata.sqlty]
dialect = "postgres"
schema = "db/schema.sql"
```

A directory with more than one file reads the first of `sqlty.toml`,
`pyproject.toml` and `Cargo.toml` that holds a sqlty table, and no other.

<!-- generated:config-keys -->
| Key | Type | Default | Used by | Summary |
|-----|------|---------|---------|---------|
| [`dialect`](#dialect) | `"postgres"`, `"sqlite"`, `"mysql"` | `"postgres"` | `rewrite`, `check` | SQL dialect of queries and schema |
| [`schema`](#schema) | string or list of strings | none | `rewrite`, `check` | Schema `.sql` files, directories or globs, relative to the configuration file |
| [`queries`](#queries) | string or list of strings | none | `check` | `.sql` files of named queries (aiosql, sqlc, PugSQL), directories or globs, relative to the configuration file |
| [`query-format`](#query-format) | `"auto"`, `"aiosql"`, `"sqlc"`, `"pugsql"`, `"file"` | `"auto"` | `check` | How `queries` files name their queries; `auto` reads each file's first header |
| [`rules`](#rules) | table of rule = severity | per-rule defaults | `check` | Severity per lint rule: `"error"`, `"warn"` or `"ignore"` |
| [`overrides`](#overrides) | array of tables | `[]` | `check` | Rule severities for the files `include` matches and `exclude` does not, on top of `rules` |
| [`import-style`](#import-style) | string | `"from sqlty import SQL"` | `rewrite`, `strip`, `check`, magic codec | How rewritten code imports and spells the wrapper |
| [`untyped`](#untyped) | `"skip"`, `"sentinel"` | `"skip"` | `rewrite` | Whether queries of unknown row type are wrapped as `SQL[UNTYPED]` |
| [`json-types`](#json-types) | `"typing.Any"`, `"typing.TypedDict"` | `"typing.Any"` | `rewrite`, `check --fix` | How a JSON column of known shape is typed: `typing.Any`, or a generated `TypedDict` |
| [`param-types`](#param-types) | `true`, `false` | `false` | `rewrite`, `check --fix` | Whether `SQL[R]` calls get the type of their parameters, `SQL[R, P]` |
| [`preview`](#preview) | boolean | `false` | `check` | Turn on features still in preview: query variants |
| [`max-query-variants`](#max-query-variants) | positive integer | `64` | `check` | With `preview`, the most variants of a query built in branches that `check` checks |
| [`functions`](#functions) | array of tables | `[]` | `check` | Helper functions whose string argument is a query |
| [`types`](#types) | table of SQL type or `table.column` = Python type | `{}` | `rewrite`, `check` | Python types a converter above the driver gives a SQL type or a column |
| [`format-command`](#format-command) | list of strings | none | `rewrite`, `check --fix` | Formatter run on each file `rewrite` or `check --fix` changes, with the path appended |
| [`extend`](#extend) | string | none | `rewrite`, `strip`, `check`, magic codec | Another configuration file this one is applied on top of, relative to this file |
| [`databases`](#databases) | table of tables | `{}` | `rewrite`, `check` | Named databases, each with its own `dialect`, `schema` and the files it applies to |
| [`exclude`](#exclude-and-extend-exclude) | list of globs | see [Default excludes](#exclude-and-extend-exclude) | `rewrite`, `strip`, `check` | Globs skipped while walking a directory; replaces the default list |
| [`extend-exclude`](#exclude-and-extend-exclude) | list of globs | `[]` | `rewrite`, `strip`, `check` | Globs skipped in addition to `exclude` or the defaults |
| [`respect-gitignore`](#respect-gitignore) | boolean | `true` | `rewrite`, `strip`, `check` | Skip the files `.gitignore` ignores while walking a directory |
<!-- /generated:config-keys -->

## `dialect`

SQL dialect for queries and the schema: `"postgres"` (default), `"sqlite"`,
`"mysql"`, or another [dialect](engine.md#dialects) sqlparser-rs knows. CLI:
`--dialect` (the first three only), which overrides this key. See
[Dialects](../guide/schema-inference.md#dialects).

## `schema`

A `.sql` file with `CREATE TABLE` statements, or a list of them, relative to
the directory of the configuration file. Each entry is a file, a directory (every
`*.sql` below it) or a glob (`tables/*.sql`, `**/*.sql`). The files are joined
in the order of the list; directory and glob matches are sorted by path, and a
file named twice is read once. CLI: `--schema`, repeatable (relative to the
current directory). See [Schema inference](../guide/schema-inference.md).

```toml
[tool.sqlty]
schema = ["extensions.sql", "tables/"]
```

## `queries`

`.sql` files of named queries that `sqlty check` reads, relative to the directory
of the configuration file: files, directories (every `*.sql` below them) and
globs, as in `schema`. Kept apart from `schema`, so a migration is never read as
a query. See [Queries in `.sql` files](../guide/sql-files.md).

```toml
[tool.sqlty]
schema = "migrations/"
queries = ["sql/**/*.sql"]
```

## `query-format`

How the `queries` files name their queries: `"aiosql"` (`-- name: get-user^`),
`"sqlc"` (`-- name: GetUser :one`), `"pugsql"` (`-- :name get_user :one`) or
`"file"` (no header: one query per file). `"auto"`, the default, reads each
file's first header, and picks `"file"` when it has none.

## `rules`

Severity per [lint rule](rules.md): `"error"`, `"warn"` or `"ignore"`.
An unknown rule name or severity is an error.

```toml
[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

## `overrides`

Rule severities for some paths only, such as tests or migrations:

```toml
[tool.sqlty.rules]
select-star = "error"

[[tool.sqlty.overrides]]
include = ["tests/**", "conftest.py"]

[tool.sqlty.overrides.rules]
select-star = "ignore"
```

The table follows ty's
[`[[tool.ty.overrides]]`](https://docs.astral.sh/ty/reference/configuration/#overrides):
`include` and `exclude` are lists of globs, and `rules` takes the same
severities as [`rules`](#rules). An entry applies to the files `include`
matches (every file when it is left out) and `exclude` does not:

```toml
[[tool.sqlty.overrides]]
include = ["generated/**"]
exclude = ["generated/important.py"]

[tool.sqlty.overrides.rules]
select-star = "ignore"
```

Globs are relative to the directory of the configuration file and match like
[`exclude`](#exclude-and-extend-exclude): a glob without a slash matches a file
or directory name at any depth, and a matching directory covers every file in
it. Every entry that matches a file is merged over `rules`, later entries
winning. `--error`, `--warn`, `--ignore` and `-c rules.X=...` still win over
both.

An override changes severities only; `dialect`, `schema` and the other keys
apply to the whole project. `check` and `check --fix` read it.

## `import-style`

How rewritten code imports and spells the wrapper. Default:
`"from sqlty import SQL"`.

| Value | Wrapper |
|-------|---------|
| `"from sqlty import SQL"` | `SQL[T](...)` |
| `"from sqlty import SQL as Q"` (any alias) | `Q[T](...)` |
| `"import sqlty"` | `sqlty.SQL[T](...)` |
| `"import sqlty as sy"` (any alias) | `sy.SQL[T](...)` |

`strip` and `check` use the same setting to recognize wrappers, so change it
only on stripped sources. With `untyped = "sentinel"`, `UNTYPED` follows the
style: `from sqlty import UNTYPED`, or `sqlty.UNTYPED` (`sy.UNTYPED`) for
module imports.
Example: [`examples/05_import_style/`](https://github.com/jirikuncar/sqlty/tree/main/examples/05_import_style).

## `preview`

`true` turns on features that may still change before they are on by default:

- [Query variants](../guide/query-variants.md): a query built from names bound in
  branches is checked once per path that builds it, instead of being skipped as
  `dynamic-query`.

```toml
[tool.sqlty]
preview = true
```

## `max-query-variants`

With `preview`, the most variants of one query that `check` checks, `64` by default. A
query with more is reported as `dynamic-query`, or `unchecked-query` for a
`psycopg.sql` composition, with the count: it is never checked on part of its variants.

```toml
[tool.sqlty]
preview = true
max-query-variants = 256
```

## `untyped`

`"skip"` (default) leaves row-returning queries of unknown type as plain
strings. `"sentinel"` wraps them as `SQL[UNTYPED](...)`. CLI:
`sqlty rewrite --untyped`. See [Untyped queries](../guide/untyped.md).

## `json-types`

`"typing.Any"` (default) types a JSON column `typing.Any`. `"typing.TypedDict"` makes
`rewrite` and `check --fix` type a JSON column whose shape the query spells out
with generated `TypedDict`s, written after the imports under a
`# sqlty: generated` line. `strip` removes them. See
[Generated TypedDicts](../guide/json-shapes.md#generated-typeddicts).

```toml
[tool.sqlty]
json-types = "typing.TypedDict"
```

## `param-types`

`false` (default) leaves the parameters of a query untyped, `SQL[R]`. `true`
makes `rewrite` and `check --fix` write their type, `SQL[R, P]`: a `tuple`, or
a generated `TypedDict` for named parameters, written as `json-types` writes
its classes. `strip` removes them. See
[Typed parameters](../guide/parameters.md#typed-parameters).

```toml
[tool.sqlty]
param-types = true
```

Example: [`examples/06_param_types/`](https://github.com/jirikuncar/sqlty/tree/main/examples/06_param_types).

!!! warning "Type checkers become stricter than the driver"
    A type checker then rejects calls that psycopg runs: a list for a `tuple`,
    text for another type, a `dict[str, Any]` or extra keys for a `TypedDict`.
    That is why it is off by default.

## `functions`

An array of tables registering helper functions. `sqlty check` checks their
calls like those of an [annotated helper](../guide/helpers.md#helpers-you-cannot-annotate)
without editing the signature (a method is `"pkg.db.Db.fetch_all"`):

```toml
[[tool.sqlty.functions]]
import-path = "db.fetch_all"
query-param = "query"
```

<!-- generated:config-function-keys -->
| Key | Required | Description |
|-----|----------|-------------|
| `import-path` | yes | Module path and function name, for example `"app.db.sql"`; a method is `"pkg.db.Db.fetch_all"`. A package that re-exports it, `pkg.Db.fetch_all`, works too. Use `"__main__.sql"` or the file name for a script run directly |
| `query-param` | yes | Name of the parameter that holds the query |
| `return-type-param` | no | Parameter whose argument, such as `return_tp=int`, is the row type of that call instead of an inferred one; `check` compares it with the query columns |
<!-- /generated:config-function-keys -->

Entries without `import-path` or `query-param` are skipped with a warning.

## `types`

The Python type a converter above the driver gives a SQL type or a column. sqlty
types the value the [reference driver](types.md) returns; when your code reads
values through something that converts them (an ORM, `sqlite3` with
`detect_types`), say what it returns:

```toml
[tool.sqlty.types]
DATE = "datetime.date"
TIMESTAMP = "datetime.datetime"
BOOLEAN = "bool"
"events.duration" = "datetime.timedelta"
```

A key with a dot is `table.column`; any other is a SQL type name, read in the
project's dialect (`NUMERIC` covers `DECIMAL(10, 2)`). The type applies to
schema columns and casts; an expression over a column keeps its type when it
returns the column's values (`MAX(day)`). See
[Drivers and ORMs](../guide/drivers-and-orms.md#what-to-annotate).

`ARRAY = "list"` types every array column as a list of its element's type, with
the element's own override: in SQLite, where an ORM decodes the JSON text it
stores, `INT[]` and `ARRAY_DATE` become `list[int]` and `list[datetime.date]`
with `DATE = "datetime.date"`.

`rewrite` and `check --fix` import the module a type names (`import app.models` for
`app.models.Status`), as they import `datetime`. `strip` leaves that import, since
it reads no configuration.

## `format-command`

A formatter `rewrite` and `check --fix` run on each file they change, with the
file's path appended to the command. A long `SQL[tuple[...]]` wrapper then
leaves the file formatted, instead of failing your formatter's check until it
runs again.

```toml
[tool.sqlty]
format-command = ["ruff", "format", "--quiet"]
```

Files that are not changed are never formatted, and `--check` and `--diff`
write nothing, so they run no formatter. A formatter that fails is reported as
a warning and the file keeps sqlty's changes. `rewrite` stays idempotent with a
formatter: a wrapper the formatter split over several lines is recognized as
already written.

`rewrite` adds its imports (`from sqlty import SQL`, `import datetime`) at the top of
the import block, below the module docstring, without sorting them into your groups.
To keep an import sorter's check passing, sort them as part of the command, for example
with a script:

```bash
#!/bin/sh
# scripts/format.sh: sort imports, then format
ruff check --select I --fix --quiet "$@" && ruff format --quiet "$@"
```

```toml
[tool.sqlty]
format-command = ["sh", "scripts/format.sh"]
```

## `extend`

A package in a monorepo keeps its own configuration and shares the rest with the root.

`packages/billing/pyproject.toml`:

```toml
[tool.sqlty]
extend = "../../pyproject.toml"
schema = "db/billing.sql"

[tool.sqlty.rules]
select-star = "error"
```

The package's settings are applied on top of the extended file's:

- `rules` and `types` merge key by key, so the package adds or changes single entries.
- Any other key the package sets replaces the extended one, `functions` and `overrides` included.
- `schema` and `queries` of the extended file keep pointing at the files it names.
- `exclude`, `extend-exclude` and `overrides` globs match paths relative to the package's file.

`extend` is relative to the file that holds it, and the extended file may extend another.
It reads `sqlty.toml`, `pyproject.toml` or `Cargo.toml` as the upward search does. A
missing file, or a chain that comes back to a file already in it, is an error.

!!! note "The editor reloads on its own files only"
    `sqlty server` reloads when a `sqlty.toml`, `pyproject.toml` or `Cargo.toml` changes.
    An extended file of another name takes effect when the server restarts.

## `databases`

One package can query several databases, each with its own dialect and schema. Name each
one under `databases`; the top-level keys stay the default database.

```toml
[tool.sqlty]
schema = "db/app.sql"

[tool.sqlty.databases.analytics]
dialect = "sqlite"
schema = "db/analytics.sql"
queries = ["sql/analytics"]
include = ["src/app/reports"]
exclude = ["legacy.py"]

[tool.sqlty.databases.analytics.rules]
select-star = "ignore"
```

A file is checked against the first database that names it:

1. A `.sql` file its `queries` names.
2. A file its `include` globs match and its `exclude` globs don't, matched as
   [`overrides`](#overrides) match.
3. Any other file uses the default database.

A database takes `dialect`, `schema`, `queries`, `query-format`, `rules`, `types` and
`functions`. A key it leaves unset is the top-level one; `rules` and `types` merge key by
key, so a database changes single entries. Every other setting, such as `import-style`,
applies to all databases. Its paths and globs are relative to the configuration file.

`--dialect` and `--schema` apply to the default database and to a database that names
neither. A database name is letters, digits, `-` and `_`; `default` is reserved.

!!! note "The magic codec reads the default database"
    `# coding: sqlty` decodes a module with the top-level settings, whatever database its
    path belongs to. [#985](https://github.com/jirikuncar/sqlty/issues/985) tracks naming
    the database in the declaration.

### Name the database in the query

A query string names its database on the marker line, and a `.sql` file in a header
line above its first query. The name overrides the file's database for that query.

```toml
[tool.sqlty]
schema = "app.sql"

[tool.sqlty.databases.analytics]
dialect = "sqlite"
schema = "analytics.sql"
```

`app.sql`:

```sql
CREATE TABLE users (id TEXT NOT NULL);
```

`analytics.sql`:

```sql
CREATE TABLE events (n INTEGER NOT NULL);
```

```python
users = "--sql\nSELECT id FROM users"
events = "--sql sqlty:db=analytics\nSELECT n FROM events"
# rewritten: users = SQL[tuple[str]]("--sql\nSELECT id FROM users")
# rewritten: events = SQL[tuple[int]]("--sql sqlty:db=analytics\nSELECT n FROM events")
```

```sql
-- sqlty: db=analytics

-- name: count_events
-- error:
SELECT COUNT(*) FROM events;
```

The query is read with that database's `dialect`, `schema` and `types`, and
`db=default` names the top-level one. Its `rules` stay those of the file. Everything
after `--sql` on the marker line is a SQL comment, so the database still runs the query
as written; under MySQL, write `-- sql sqlty:db=analytics`. A name the project does not
configure is [`unknown-database`](rules.md#unknown-database).

### Helpers of a database

A helper a database lists in its `functions` checks its calls against that database, in
any file. You can keep a warehouse client next to the application code, without moving
its callers under the database's `include`.

```toml
[tool.sqlty]
schema = "app.sql"

[tool.sqlty.databases.analytics]
dialect = "sqlite"
schema = "analytics.sql"

[[tool.sqlty.databases.analytics.functions]]
import-path = "app.warehouse.fetch"
query-param = "query"
return-type-param = "form"
```

`app.sql`:

```sql
CREATE TABLE users (id TEXT NOT NULL);
```

`analytics.sql`:

```sql
CREATE TABLE events (n INTEGER NOT NULL);
```

`app/warehouse.py`:

```python
def fetch[T](query: str, form: type[T]) -> list[T]: ...
```

```python
from app.warehouse import fetch

# error: [column-type-mismatch] "tuple[str] item 0 is str but query column 'n' infers int"
fetch("SELECT n FROM events", tuple[str])
```

A query that names its database, `--sql sqlty:db=default` say, keeps it. Completion and
hover in a helper's query still read the file's database.

## `exclude` and `extend-exclude`

Directories the CLI walks skip these globs. `exclude` replaces the default
list; `extend-exclude` adds to whichever list applies.

The default excludes are:

<!-- generated:config-default-excludes -->
`.bzr`, `.direnv`, `.eggs`, `.git`, `.git-rewrite`, `.hg`, `.ipynb_checkpoints`, `.mypy_cache`, `.nox`, `.pants.d`, `.pyenv`, `.pytest_cache`, `.pytype`, `.ruff_cache`, `.svn`, `.tox`, `.venv`, `.vscode`, `__pypackages__`, `_build`, `buck-out`, `dist`, `node_modules`, `site-packages`, `venv`
<!-- /generated:config-default-excludes -->

A glob without a slash matches a file or
directory name at any depth; one with a slash matches the path relative to
the configuration file that holds it, and `**` recurses. Files named on the
command line are always processed, unless `--force-exclude` is given. The CLI's `--exclude` and
`--extend-exclude` take the same globs, relative to the current directory.

```toml
[tool.sqlty]
extend-exclude = ["migrations", "gen/*.py"]
```

## `respect-gitignore`

Directories the CLI walks also skip what git ignores: every `.gitignore` from
the repository root down, and `.git/info/exclude`, with negations (`!keep.py`)
and nested files read as git reads them. Build output and generated code are
usually listed there already, so they need no `exclude` entry. Outside a git
repository nothing is skipped this way; the global `core.excludesFile` is not
read.

```toml
[tool.sqlty]
respect-gitignore = false
```

Files named on the command line are always processed. The CLI's
`--respect-gitignore` and `--no-respect-gitignore` override the setting.
