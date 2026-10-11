# CLI

```
sqlty [-h] [--version] {rewrite,strip,check,rule,version} ...
```

```bash
sqlty rewrite .   # "--sql\n..." -> SQL[T]("--sql\n...")
sqlty check .     # lint row types, drift, SELECT *
sqlty check --fix .  # apply the fixable rules, report the rest
sqlty strip .     # SQL[T]("--sql\n...") -> "--sql\n..."
```

Each command takes files or directories (default: the current directory) and
processes every `.py` file under them. Each file uses `[tool.sqlty]` from the
nearest `pyproject.toml` above it that has one; see
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
imports the types need. Implicitly concatenated strings are wrapped as one
query. Existing wrappers are left alone. `--sql` strings that are not plain
literals (f-, t-, raw or bytes strings, alone or in a concatenation) are
skipped and reported on stderr (`fstring-skipped`, `tstring-skipped`,
`unchecked-query`) unless `-q` is given or the rule is set to `ignore`; they
never change the exit code.

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
hand-written row types such as `SQL[User]`, and the sqlty import line. When the
strings can't stand alone (a comment outside them, or line breaks that only the
call's parentheses allow), the parentheses stay: `SQL[T]("..."  # c\n)` becomes
`("..."  # c\n)`. A wrapper whose argument is not only string literals
(`SQL[T]("--sql..." + x)`) is left in place, as is a dotted `mod.SQL[T](...)`,
which `check` still checks.

```
sqlty strip [-h] [--dialect {postgres,sqlite,mysql}] [--schema SCHEMA]
            [-v] [-q] [--check] [--diff]
            [paths ...]
```

| Option | Description |
|--------|-------------|
| `--check` | Do not modify files; print `Would strip <file>` and exit 1 if any file would change |
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
| `--fix` | Apply the [fixable rules](rules.md), write the files, then report what remains |

Rule severities come from `[tool.sqlty.rules]`; see [Rules](rules.md).

Exit code: 1 if any diagnostic has severity `error`, otherwise 0.

## `sqlty rule`

Print a rule's documentation, or list every rule with its default severity.

```
sqlty rule [-h] [name]
```

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
