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
processes every `.py` file under them, skipping the default and configured
[excludes](configuration.md#exclude-and-extend-exclude) (`.venv`, `.git`,
`node_modules`, ...) while walking a directory; a file named on the command
line is always processed. A path that does not exist is an error. Each file uses `[tool.sqlty]` from the
nearest `pyproject.toml` above it that has one; see
[Configuration](configuration.md). Command-line flags override it.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Nothing to report: no `error` diagnostic from `check`, no file changed by `--check` or `--diff`, or the files were rewritten or stripped |
| 1 | `check` found an `error`-severity diagnostic, or `rewrite` / `strip` with `--check` or `--diff` found files that would change |
| 2 | Usage or configuration error (unknown option, unknown key or rule in `[tool.sqlty]`, a path that does not exist) or an internal error |

## Common options

`rewrite`, `strip` and `check` accept:

| Option | Description |
|--------|-------------|
| `paths ...` | Files or directories to process (default: `.`) |
| `--dialect {postgres,sqlite,mysql}` | SQL dialect; overrides `dialect` in `pyproject.toml` (default: that key, else `postgres`) |
| `--schema SCHEMA` | Schema `.sql` file, directory or glob; repeat for several, joined in order. Overrides `schema` in `pyproject.toml` |
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
| `--diff` | Print a unified diff instead of modifying files; exit 1 if any file would change |
| `--untyped {skip,sentinel}` | Row-returning queries of unknown row type: leave unwrapped (`skip`, default) or wrap as `SQL[UNTYPED](...)` (`sentinel`). Overrides `untyped` in `pyproject.toml` |

Exit code: see [Exit codes](#exit-codes); 1 with `--check` or `--diff` when files would change.

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
| `--diff` | Print a unified diff instead of modifying files; exit 1 if any file would change |

Exit code: see [Exit codes](#exit-codes); 1 with `--check` or `--diff` when files would change.

## `sqlty check`

Lint `--sql` strings and `SQL[T](...)` wrappers. Prints one line per
diagnostic and a summary:

```
queries.py:2: [stale-type] SQL[tuple[int, str]] should be SQL[tuple[int, str, str]]
Found 1 warning(s)
```

```
sqlty check [-h] [--dialect {postgres,sqlite,mysql}] [--schema SCHEMA]
            [-v] [-q] [--fix] [--statistics]
            [paths ...]
```

| Option | Description |
|--------|-------------|
| `--fix` | Apply the [fixable rules](rules.md), write the files, then report what remains |
| `--statistics` | After the summary, print how many queries were typed, partially typed, unparsed and skipped |

With `--statistics`, the last line counts the queries of the checked files:

```
Queries: 1 typed, 1 partially typed, 1 unparsed, 1 skipped (fstring-skipped: 1)
```

A query is typed when every column of its row type is known, and partially
typed when some column's type is not (`SELECT *` with no schema, an unknown
column). Unparsed queries are those the parser rejects, whether or not
[`unparsed-query`](rules.md#unparsed-query) is enabled. Skipped strings are
f-, t- and raw strings, and other `--sql` strings that are not plain literals,
counted by the rule that reports them. Queries that return no rows are not
counted.

Rule severities come from `[tool.sqlty.rules]`; see [Rules](rules.md).

Exit code: 1 if any diagnostic has severity `error`; warnings alone exit 0.

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

## Environment variables

| Variable | Effect |
|----------|--------|
| `SQLTY_NO_MAGIC` | Any non-empty value (even `0`) stops `sqlty.magic` from registering the magic codec on import |
