# Configuration

sqlty reads `[tool.sqlty]` from `pyproject.toml`. The CLI uses, for each
file, the nearest `pyproject.toml` above it, unless
[`--config-file` or `--isolated`](cli.md#configuration-flags) picks one file or none; the magic codec
searches from the current directory. Command-line flags override the file. An unknown key, rule or
severity is an error (exit 2) that names the file and suggests the closest match.

In every case a `pyproject.toml` without `[tool.sqlty]` is skipped, so a
package's packaging-only `pyproject.toml` in a monorepo does not hide the
root config. The closest table wins; settings are not merged with tables in
parent directories. To change one setting for a nested project, give it its
own complete `[tool.sqlty]` table. Command-line flags (`--dialect`,
`--schema`, `--untyped`, `--exclude`, `--extend-exclude`, `--error`/`--warn`/`--ignore`)
and `-c KEY=VALUE` assignments override the table for every path.

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

<!-- generated:config-keys -->
| Key | Type | Default | Used by | Summary |
|-----|------|---------|---------|---------|
| [`dialect`](#dialect) | `"postgres"`, `"sqlite"`, `"mysql"` | `"postgres"` | `rewrite`, `check` | SQL dialect of queries and schema |
| [`schema`](#schema) | string or list of strings | none | `rewrite`, `check` | Schema `.sql` files, directories or globs, relative to `pyproject.toml` |
| [`rules`](#rules) | table of rule = severity | per-rule defaults | `check` | Severity per lint rule: `"error"`, `"warn"` or `"ignore"` |
| [`overrides`](#overrides) | array of tables | `[]` | `check` | Rule severities for the files `include` matches and `exclude` does not, on top of `rules` |
| [`import-style`](#import-style) | string | `"from sqlty import SQL"` | `rewrite`, `strip`, `check`, magic codec | How rewritten code imports and spells the wrapper |
| [`untyped`](#untyped) | `"skip"`, `"sentinel"` | `"skip"` | `rewrite` | Whether queries of unknown row type are wrapped as `SQL[UNTYPED]` |
| [`json-types`](#json-types) | `"typing.Any"`, `"typing.TypedDict"` | `"typing.Any"` | `rewrite`, `check --fix` | How a JSON column of known shape is typed: `typing.Any`, or a generated `TypedDict` |
| [`type-modules`](#type-modules) | list of strings | `[]` | `check` | Modules `check` may import to resolve row type fields |
| [`preview`](#preview) | boolean | `false` | `check` | Turn on features still in preview: query variants, Pydantic types |
| [`max-query-variants`](#max-query-variants) | positive integer | `64` | `check` | With `preview`, the most variants of a query built in branches that `check` checks |
| [`functions`](#functions) | array of tables | `[]` | `check` | Helper functions whose string argument is a query |
| [`types`](#types) | table of SQL type or `table.column` = Python type | `{}` | `rewrite`, `check` | Python types a converter above the driver gives a SQL type or a column |
| [`format-command`](#format-command) | list of strings | none | `rewrite`, `check --fix` | Formatter run on each file `rewrite` or `check --fix` changes, with the path appended |
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
the directory of `pyproject.toml`. Each entry is a file, a directory (every
`*.sql` below it) or a glob (`tables/*.sql`, `**/*.sql`). The files are joined
in the order of the list; directory and glob matches are sorted by path, and a
file named twice is read once. CLI: `--schema`, repeatable (relative to the
current directory). See [Schema inference](../guide/schema-inference.md).

```toml
[tool.sqlty]
schema = ["extensions.sql", "tables/"]
```

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

Globs are relative to the `pyproject.toml` directory and match like
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

## `type-modules`

Modules `check` may import to resolve the types of row type fields. By default
only the standard library and `typing`, `typing_extensions`, `pydantic`,
`pydantic_core`, `annotated_types` and `sqlty` are imported; names from
modules of the checked paths are read from their source, and any other name is
unknown and never reported, so no import-time code of your application runs.
List a module (its submodules are included) to have it imported:

```toml
[tool.sqlty]
type-modules = ["myapp.types"]
```

## `preview`

`true` turns on features that may still change before they are on by default:

- [Query variants](../guide/query-variants.md): a query built from names bound in
  branches is checked once per path that builds it, instead of being skipped as
  `dynamic-query`.
- [Pydantic types](../guide/row-types.md#pydantic-types) are read from a table, not
  from pydantic: a model field takes what Pydantic validates into it.

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
| `import-path` | yes | Module path and function name, for example `"app.db.sql"`; a method is `"pkg.db.Db.fetch_all"`. Use `"__main__.sql"` or the file name for a script run directly |
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

## `exclude` and `extend-exclude`

Directories the CLI walks skip these globs. `exclude` replaces the default
list; `extend-exclude` adds to whichever list applies.

The default excludes are:

<!-- generated:config-default-excludes -->
`.bzr`, `.direnv`, `.eggs`, `.git`, `.git-rewrite`, `.hg`, `.ipynb_checkpoints`, `.mypy_cache`, `.nox`, `.pants.d`, `.pyenv`, `.pytest_cache`, `.pytype`, `.ruff_cache`, `.svn`, `.tox`, `.venv`, `.vscode`, `__pypackages__`, `_build`, `buck-out`, `dist`, `node_modules`, `site-packages`, `venv`
<!-- /generated:config-default-excludes -->

A glob without a slash matches a file or
directory name at any depth; one with a slash matches the path relative to
the `pyproject.toml` that holds it, and `**` recurses. Files named on the
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
