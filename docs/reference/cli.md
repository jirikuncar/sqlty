# CLI

```
sqlty [-h] [--version] {rewrite,strip,check,version} ...
```

```bash
sqlty rewrite .   # "--sql\n..." -> SQL[T]("--sql\n...")
sqlty check .     # lint row types, drift, SELECT *
sqlty strip .     # SQL[T]("--sql\n...") -> "--sql\n..."
```

Each command takes files or directories (default: the current directory) and
processes every `.py` file under them. Settings come from the nearest
`pyproject.toml` with a `[tool.sqlty]` table above each file; see
[Configuration](configuration.md). Command-line flags override it.

## Common options

`rewrite`, `strip` and `check` accept:

| Option | Description |
|--------|-------------|
| `paths ...` | Files or directories to process (default: `.`) |
| `--dialect {postgres,sqlite,mysql}` | SQL dialect; overrides `dialect` in `pyproject.toml` (default: that key, else `postgres`) |
| `--schema SCHEMA` | Schema `.sql` file; overrides `schema` in `pyproject.toml` |
| `-v`, `--verbose` | Print each file `rewrite` or `strip` changes |
| `-q`, `--quiet` | Suppress non-error output |
| `-h`, `--help` | Show help and exit |

`strip` accepts `--dialect` and `--schema` but does not use them.

## `sqlty rewrite`

Wrap `--sql` strings in `SQL[T](...)` with the inferred row type and add the
imports the types need. Existing wrappers are left alone.

```
sqlty rewrite [-h] [--dialect {postgres,sqlite,mysql}] [--schema SCHEMA]
              [-v] [-q] [--check] [--diff] [--untyped {skip,sentinel}]
              [paths ...]
```

| Option | Description |
|--------|-------------|
| `--check` | Do not modify files; print `Would rewrite <file>` and exit 1 if any file needs rewriting |
| `--diff` | Print a unified diff instead of modifying files |
| `--untyped {skip,sentinel}` | Row-returning queries of unknown row type: leave unwrapped (`skip`, default) or wrap as `SQL[UNTYPED](...)` (`sentinel`). Overrides `untyped` in `pyproject.toml` |

Exit code: 0, or 1 with `--check` when files would change.

## `sqlty strip`

Remove `SQL[...](...)` wrappers around `--sql` strings, including
hand-written row types such as `SQL[User]`, and the sqlty import line.

```
sqlty strip [-h] [--dialect {postgres,sqlite,mysql}] [--schema SCHEMA]
            [-v] [-q] [--check] [--diff]
            [paths ...]
```

| Option | Description |
|--------|-------------|
| `--check` | Do not modify files; print `Would strip <file>` and exit 1 if any file contains wrappers |
| `--diff` | Print a unified diff instead of modifying files |

Exit code: 0, or 1 with `--check` when files would change.

## `sqlty check`

Lint `--sql` strings and `SQL[T](...)` wrappers. Prints one line per
diagnostic and a summary:

```
queries.py:2: [stale-type] SQL[tuple[int, str]] should be SQL[tuple[int, str, str]]
Found 1 warning(s)
```

```
sqlty check [-h] [--dialect {postgres,sqlite,mysql}] [--schema SCHEMA]
            [-v] [-q] [--fix]
            [paths ...]
```

| Option | Description |
|--------|-------------|
| `--fix` | Accepted, but currently does not change files |

Rule severities come from `[tool.sqlty.rules]`; see [Rules](rules.md).

Exit code: 1 if any diagnostic has severity `error`, otherwise 0.

## `sqlty version`

Print `sqlty <version>` and exit. `sqlty --version` does the same.

## Other entry points

| Command | Description |
|---------|-------------|
| `python -m sqlty.magic FILE` | Print the source the [magic codec](../guide/magic-codec.md) produces for `FILE` |
| `python -m sqlty.registries [PATH]` | List functions registered in `[[tool.sqlty.functions]]` found under `PATH`, as `file:function` |

## Environment variables

| Variable | Effect |
|----------|--------|
| `SQLTY_AUTO_GENERATE=1` | Generate `.pyi` stubs for registered `@sqlty` helpers at runtime (`1`, `true`, `yes`); see [Generated stubs](../guide/stubs.md) |
| `SQLTY_NO_MAGIC` | Any non-empty value (even `0`) stops `sqlty.magic` from registering the magic codec on import |
| `SQLTY_LIB=py` / `SQLTY_LIB=rs` | Force the pure-Python or the Rust backend; see [Backends](backends.md) |
