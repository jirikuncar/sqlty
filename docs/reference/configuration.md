# Configuration

sqlty reads `[tool.sqlty]` from `pyproject.toml`. The CLI uses, for each
file, the nearest `pyproject.toml` above it; the magic codec searches from the
current directory, and stub generation from the directory of the helper's
module. Command-line flags override the file. Unknown keys are ignored.

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
import_path = "db.sql"
query_param = "query"
```

| Key | Used by |
|-----|---------|
| [`dialect`](#dialect) | `rewrite`, `check`, stub generation |
| [`schema`](#schema) | `rewrite`, `check`, stub generation |
| [`schemas`](#schemas) | Stub generation |
| [`rules`](#rules) | `check` |
| [`import-style`](#import-style) | `rewrite`, `strip`, `check`, magic codec |
| [`untyped`](#untyped) | `rewrite`, stub generation |
| [`format-command`](#format-command) | Stub generation |
| [`functions`](#functions) | Stub generation, `python -m sqlty.registries` |

## `dialect`

SQL dialect for queries and the schema: `"postgres"` (default), `"sqlite"`
or `"mysql"`. CLI: `--dialect`. Because `postgres` is the flag's default, an
explicit `--dialect postgres` does not override this key. See
[Dialects](../guide/schema-inference.md#dialects).

## `schema`

Path to a `.sql` file with `CREATE TABLE` statements, relative to the
directory of `pyproject.toml`. CLI: `--schema` (relative to the current
directory). See [Schema inference](../guide/schema-inference.md).
Stub generation uses it for helpers that have no more specific schema; see
[Generated stubs](../guide/stubs.md#schemas).

## `schemas`

Schema files for [stub generation](../guide/stubs.md#schemas), keyed by the
module that defines the helper (`"db"`) or by module and function
(`"db.sql"`). Paths are relative to the directory of `pyproject.toml`.

```toml
[tool.sqlty.schemas]
db = "db/schema.sql"                       # every helper in db.py
"analytics.query" = "db/analytics.sql"    # only analytics.query
```

Keys use the module name the helper has at runtime, so a helper in a script
run directly is under `__main__`. `rewrite` and `check` do not read this
table.

## `rules`

Severity per [lint rule](rules.md): `"error"`, `"warn"` or `"ignore"`.
Unknown rule names and invalid severities are ignored.

```toml
[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

## `import-style`

How rewritten code imports and spells the wrapper. Default:
`"from sqlty import SQL"`.

| Value | Wrapper |
|-------|---------|
| `"from sqlty import SQL"` | `SQL[T](...)` |
| `"from sqlty import SQL as Q"` (any alias) | `Q[T](...)` |
| `"import sqlty"` | `sqlty.SQL[T](...)` |
| `"import sqlty as sy"` (any alias) | `sy.SQL[T](...)` |

`strip` and `check` use the same setting to recognise wrappers, so change it
only on stripped sources. With `untyped = "sentinel"`, `UNTYPED` follows the
style: `from sqlty import UNTYPED`, or `sqlty.UNTYPED` (`sy.UNTYPED`) for
module imports.
Example: [`examples/05_import_style/`](https://github.com/jirikuncar/sqlty/tree/main/examples/05_import_style).

## `untyped`

`"skip"` (default) leaves row-returning queries of unknown type as plain
strings. `"sentinel"` wraps them as `SQL[UNTYPED](...)`, and makes the
catch-all overload of generated stubs return `SQL[UNTYPED]`. CLI:
`sqlty rewrite --untyped`. See [Untyped queries](../guide/untyped.md).

## `format-command`

A formatter command with a `{filename}` placeholder, reading the source on
stdin:

```toml
[tool.sqlty]
format-command = "ruff format --stdin-filename {filename} -"
```

Stub generation pipes each `.pyi` file it writes through the command,
with `{filename}` replaced by the stub path. If the command fails, the
unformatted stub is written and a warning is logged. `sqlty rewrite` does not
run the formatter; run yours after it.

## `functions`

An array of tables registering helper functions for
[stub generation](../guide/stubs.md):

```toml
[[tool.sqlty.functions]]
import_path = "db.fetch_all"
query_param = "query"
return_type_template = "list[{sqlty}]"
```

| Key | Required | Description |
|-----|----------|-------------|
| `import_path` | yes | Module path and function name, e.g. `"app.db.sql"`. Use `"__main__.sql"` or the file name for a script run directly |
| `query_param` | yes | Name of the parameter that holds the query |
| `return_type_template` | no | Return type of each overload, with `{sqlty}` for the inferred row type. Default: `"SQL[{sqlty}]"` |
| `imports` | no | Import lines added to the stub (a string or a list), for names used in `return_type_template` |
| `schema` | no | Schema path for this helper, relative to the directory of `pyproject.toml`. Overrides every other [schema source](../guide/stubs.md#schemas) |
| `return_type_param` | no | Parameter whose argument, such as `return_tp=int`, is used as the row type of that call instead of inferring it |
| `self_type_template` | no | Reserved for methods, which are not supported |

Entries without `import_path` or `query_param` are skipped with a warning.
