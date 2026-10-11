# SQLTy

Type-safe raw SQL for Python. sqlty infers the row type of each query from
its SQL, writes it into your source as `SQL[T]`, and checks that it stays
correct.

!!! warning "Experimental"
    sqlty is experimental. APIs and configuration may change.

## From a raw query to a typed one

Mark a query with `--sql` and cast its columns. Run `sqlty rewrite .`, and
the file becomes what the `rewritten-file` lines show:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: query = SQL[tuple[int, str, datetime.datetime]]("--sql\nSELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users")
```

Each cast becomes a Python type: `INTEGER` is `int`, `TEXT` is `str`,
`TIMESTAMP` is `datetime.datetime` (see the
[type mapping](guide/writing-queries.md#type-mapping)). With a
[schema file](guide/schema-inference.md), the casts are optional.

`SQL[T]` is a `str` subclass. The query still works anywhere a string does,
and editors and type checkers now know the row type.

## Three commands

| Command | What it does |
|---------|--------------|
| `sqlty rewrite .` | Wrap `--sql` strings in `SQL[T](...)` with the inferred type |
| `sqlty check .` | Report drift, row type mismatches, `SELECT *` and unwrapped queries |
| `sqlty strip .` | Remove the wrappers, back to plain `--sql` strings |

`rewrite` and `strip` only add or remove the wrapper and its import; the
string itself is never modified. For a file of plain `--sql` strings,
`strip(rewrite(x)) == x`. See the [Quickstart](quickstart.md).

## Features

- **[Row types](guide/row-types.md).** Use `TypedDict`, `NamedTuple`,
  dataclasses or Pydantic models instead of tuples; `sqlty check` validates
  field count, types and names. Closed TypedDicts (PEP 728) are matched by
  key, and subclasses are checked against their bases' openness.
- **[psycopg](guide/psycopg.md).** `execute(conn, SQL[User](...))` returns a
  `Cursor[User]` with the matching row factory; `scalar` fetches one typed
  value; `row_form` returns `T` at runtime.
- **[Schema inference](guide/schema-inference.md).** Read column types from
  `CREATE TABLE` statements for PostgreSQL, SQLite and MySQL.
- **[Magic codec](guide/magic-codec.md).** Files declaring `# coding: sqlty`
  are rewritten when imported, without a rewrite step.
- **[Untyped queries](guide/untyped.md).** Mark queries sqlty cannot type as
  `SQL[UNTYPED]`, which type checkers refuse to treat as a concrete row.
- **[Generated stubs](guide/stubs.md).** Type helper functions such as
  `sql("SELECT ...")` with a generated `.pyi` overload per query.
- **[Lint rules](reference/rules.md).** Thirteen rules with configurable
  severity.
- **[Rust engine](reference/engine.md).** SQL parsing, type inference and
  source rewriting in Rust, exposed to Python with PyO3.

## Which features to use

| Approach | Best for |
|----------|----------|
| [Rewrite and check](quickstart.md) | Committed code, CI, teams |
| [Row types](guide/row-types.md) | Named fields, domain objects |
| [Schema inference](guide/schema-inference.md) | Projects with `CREATE TABLE` schemas |
| [Magic codec](guide/magic-codec.md) | Scripts and prototypes |
| [psycopg helpers](guide/psycopg.md) | psycopg 3 code that wants typed rows at runtime |
| [Generated stubs](guide/stubs.md) | Code that sends queries through helper functions |

They combine: a schema plus row types plus the psycopg helpers is the most
complete setup. [Use cases](use-cases.md) describes the situations sqlty is
built for.

## Next steps

1. [Install sqlty](installation.md).
2. Follow the [Quickstart](quickstart.md).
3. Adding sqlty to an existing project? Read [Migration](recipes/migration.md)
   and [CI and pre-commit](recipes/ci.md).

New to the idea? Start with [Why SQLTy?](motivation.md). Contributing,
testing and architecture are covered in
[DEVELOPMENT.md](https://github.com/jirikuncar/sqlty/blob/main/DEVELOPMENT.md).

## Example projects

Each directory in
[`examples/`](https://github.com/jirikuncar/sqlty/tree/main/examples) has a
README:

- [`01_quick_start/`](https://github.com/jirikuncar/sqlty/tree/main/examples/01_quick_start):
  rewrite, check and strip on cast queries.
- [`02_schema_inference/`](https://github.com/jirikuncar/sqlty/tree/main/examples/02_schema_inference):
  types from a schema passed with `--schema`.
- [`03_pyproject_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/03_pyproject_config):
  schema, dialect and rules in `pyproject.toml`.
- [`04_check_and_types/`](https://github.com/jirikuncar/sqlty/tree/main/examples/04_check_and_types):
  row types validated by `sqlty check`.
- [`05_import_style/`](https://github.com/jirikuncar/sqlty/tree/main/examples/05_import_style):
  a custom `import-style`.
- [`magic_codec/`](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec):
  the magic codec.

Run them from a checkout:

```bash
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty && uv sync --all-groups
uv run sqlty check examples/01_quick_start/
uv run sqlty check --schema examples/02_schema_inference/schema.sql examples/02_schema_inference/
uv run python -m sqlty.magic examples/magic_codec/demo.py  # print the decoded source
```
