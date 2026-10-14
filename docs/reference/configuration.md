# Configuration

sqlty reads `[tool.sqlty]` from `pyproject.toml`. The CLI uses, for each
file, the nearest `pyproject.toml` above it; the magic codec searches from the
current directory. Command-line flags override the file. Unknown keys are ignored.

In every case a `pyproject.toml` without `[tool.sqlty]` is skipped, so a
package's packaging-only `pyproject.toml` in a monorepo does not hide the
root config. The closest table wins; settings are not merged with tables in
parent directories. To change one setting for a nested project, give it its
own complete `[tool.sqlty]` table. Command-line flags (`--dialect`,
`--schema`, `--untyped`) override the table for every path.

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
| [`dialect`](#dialect) | `rewrite`, `check` |
| [`schema`](#schema) | `rewrite`, `check` |
| [`rules`](#rules) | `check` |
| [`import-style`](#import-style) | `rewrite`, `strip`, `check`, magic codec |
| [`untyped`](#untyped) | `rewrite` |
| [`type-modules`](#type-modules) | `check` |
| [`functions`](#functions) | `check` ([helpers](../guide/helpers.md#helpers-you-cannot-annotate)) |

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

## `untyped`

`"skip"` (default) leaves row-returning queries of unknown type as plain
strings. `"sentinel"` wraps them as `SQL[UNTYPED](...)`. CLI:
`sqlty rewrite --untyped`. See [Untyped queries](../guide/untyped.md).

## `functions`

An array of tables registering helper functions. `sqlty check` checks their
calls like those of an [annotated helper](../guide/helpers.md#helpers-you-cannot-annotate)
without editing the signature (a method is `"pkg.db.Db.fetch_all"`):

```toml
[[tool.sqlty.functions]]
import_path = "db.fetch_all"
query_param = "query"
```

| Key | Required | Description |
|-----|----------|-------------|
| `import_path` | yes | Module path and function name, e.g. `"app.db.sql"`. Use `"__main__.sql"` or the file name for a script run directly |
| `query_param` | yes | Name of the parameter that holds the query |
| `return_type_param` | no | Parameter whose argument, such as `return_tp=int`, is used as the row type of that call instead of inferring it. `check` compares it with the query columns |

Entries without `import_path` or `query_param` are skipped with a warning.
