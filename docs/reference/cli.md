# CLI

<!-- generated:cli-usage -->
```text
sqlty [-h] [--version]
      {rewrite,strip,check,breaking,rule,skill,version} ...
```
<!-- /generated:cli-usage -->

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
line is always processed unless `--force-exclude` is given. A path that does
not exist is an error. Each file uses `[tool.sqlty]` from the
nearest `pyproject.toml` above it that has one; see
[Configuration](configuration.md). Command-line flags override it.

## Configuration flags

To run sqlty on a project without editing its `pyproject.toml`, pass the
settings on the command line or in a file of your own. The flags follow
[ty](https://docs.astral.sh/ty/reference/cli/) and
[ruff](https://docs.astral.sh/ruff/configuration/):

```bash
sqlty check --config-file ~/sqlty.toml .            # one file for every path
sqlty check -c 'schema = "db/"' -c 'dialect = "mysql"' .  # override single keys
sqlty check --isolated --schema db/ .               # defaults plus flags only
sqlty check --ignore select-star --error stale-type .  # rule severities
sqlty check --extend-exclude 'legacy/*' .           # skip more than the config does
sqlty rewrite --exclude migrations --exclude build .  # replace the excludes
```

`--config-file PATH` reads `[tool.sqlty]` from a file named `pyproject.toml` and
the same keys at the top level of any other file, such as `sqlty.toml`.

`sqlty.toml`:

```toml
schema = "db/schema"
extend-exclude = ["migrations"]

[rules]
select-star = "ignore"
```

It replaces the per-file lookup for every path, and its relative paths
(`schema`, `exclude`, `extend-exclude`) start from the current directory, so
the file can live outside the project. `SQLTY_CONFIG_FILE` sets it when
`--config-file` is not given. `--isolated` ignores every configuration file,
including `SQLTY_CONFIG_FILE`; it cannot be combined with `--config-file`.

`-c`/`--config KEY=VALUE` is a TOML assignment applied on top of whichever
configuration applies, `--isolated` included. Repeat it for several keys;
the last value of a key wins, and tables merge key by key, so
`-c 'rules.select-star = "ignore"'` keeps the other configured rules. Its
`schema`, `exclude` and `extend-exclude` are relative to the current directory,
like the flags of the same name, which take precedence.

`--error RULE`, `--warn RULE` and `--ignore RULE` set a rule's severity over
`[tool.sqlty.rules]` and `-c`; for a rule given several times, the last flag wins.

`--exclude GLOB` replaces `exclude` from the config together with the
default excludes; the config's `extend-exclude` still applies.
`--extend-exclude GLOB` adds to whichever list applies. Both repeat, and
their globs follow the rules of [`exclude`](configuration.md#exclude-and-extend-exclude),
relative to the current directory. `--force-exclude` applies the excludes to
files and directories named on the command line too, which is useful when a
tool such as pre-commit passes file names.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Nothing to report: no `error` diagnostic from `check`, no file changed by `--check` or `--diff`, or the files were rewritten or stripped |
| 1 | `check` found an `error`-severity diagnostic, `breaking` found an `error`-severity breaking change, or `rewrite` / `strip` with `--check` or `--diff` found files that would change |
| 2 | Usage or configuration error (unknown option, unknown key or rule in `[tool.sqlty]`, a path that does not exist), a Python file that is not UTF-8 (reported and skipped; the other files are still processed), or an internal error |

## Common options

`rewrite`, `strip` and `check` accept:

<!-- generated:cli-options-common -->
| Option | Description |
|--------|-------------|
| `-h`, `--help` | show this help message and exit |
| `paths ...` | Files or directories (default: .); each file uses its nearest pyproject.toml |
| `--dialect DIALECT` | SQL dialect: postgres, mysql, sqlite, or another sqlparser dialect such as duckdb (default: [tool.sqlty] dialect, else postgres) |
| `--schema SCHEMA` | Schema .sql file, directory or glob for every path; repeat for several (default: [tool.sqlty] schema per project) |
| `--config-file PATH` | Use this file for every path instead of the nearest pyproject.toml: [tool.sqlty] of a pyproject.toml, or top-level keys of another file such as sqlty.toml; its relative paths are relative to the current directory. Env: SQLTY_CONFIG_FILE |
| `--isolated` | Ignore every configuration file; use the defaults and command-line flags |
| `-c`, `--config KEY=VALUE` | Override a configuration key with a TOML assignment, for example `-c 'dialect = "mysql"'` or `-c 'rules.select-star = "ignore"'`; repeat for several. Paths are relative to the current directory |
| `--error RULE` | Treat RULE as severity error; repeat for several, the last flag for a rule wins |
| `--warn RULE` | Treat RULE as severity warn; repeat for several, the last flag for a rule wins |
| `--ignore RULE` | Treat RULE as severity ignore; repeat for several, the last flag for a rule wins |
| `--exclude GLOB` | Skip this glob while walking directories, replacing [tool.sqlty] exclude and the default excludes; repeat for several. Relative to the current directory |
| `--extend-exclude GLOB` | Skip this glob in addition to the other excludes; repeat for several. Relative to the current directory |
| `--force-exclude` | Apply the excludes to files and directories named on the command line too |
| `--respect-gitignore`, `--no-respect-gitignore` | Skip the files .gitignore ignores while walking directories (default: [tool.sqlty] respect-gitignore, else true) |
| `-v`, `--verbose` | Print each file that rewrite, strip or check --fix changes |
| `-q`, `--quiet` | Suppress non-error output |
| `-j`, `--jobs N` | Files to work on at once; 1 works on one at a time. Output is the same in any case (default: the number of processors) |
<!-- /generated:cli-options-common -->

`strip` accepts `--dialect` and `--schema` but does not use them.

sqlty reads Python files as UTF-8. A file it changes keeps its byte order mark
and its line endings, `\n` or `\r\n`, as the first line has them.

## `sqlty rewrite`

Wrap `--sql` strings in `SQL[T](...)` with the inferred row type and add the
imports the types need. Implicitly concatenated strings are wrapped as one
query. Existing wrappers are left alone. `--sql` strings that are not plain
literals (f-, t-, raw or bytes strings, alone or in a concatenation) are
skipped and reported on stderr (`fstring-skipped`, `tstring-skipped`,
`unchecked-query`) unless `-q` is given or the rule is set to `ignore`; they
never change the exit code.

<!-- generated:cli-usage-rewrite -->
```text
sqlty rewrite [-h] [--dialect DIALECT] [--schema SCHEMA]
              [--config-file PATH | --isolated] [-c KEY=VALUE]
              [--error RULE] [--warn RULE] [--ignore RULE]
              [--exclude GLOB] [--extend-exclude GLOB]
              [--force-exclude]
              [--respect-gitignore | --no-respect-gitignore] [-v] [-q]
              [-j N] [--check] [--diff] [--untyped {skip,sentinel}]
              [paths ...]
```
<!-- /generated:cli-usage-rewrite -->

<!-- generated:cli-options-rewrite -->
| Option | Description |
|--------|-------------|
| `--check` | Do not modify files; print `Would rewrite <file>` and exit 1 if any file needs rewriting |
| `--diff` | Print a unified diff instead of modifying files; exit 1 if any file would change |
| `--untyped {skip,sentinel}` | Row-returning queries of unknown row type: leave unwrapped (skip, default) or wrap as `SQL[UNTYPED](...)` (sentinel). Config: untyped in [tool.sqlty] |
<!-- /generated:cli-options-rewrite -->

Exit code: see [Exit codes](#exit-codes); 1 with `--check` or `--diff` when files would change.

## `sqlty strip`

Remove `SQL[...](...)` wrappers around `--sql` strings, including
hand-written row types such as `SQL[User]`, and the sqlty import line. When the
strings can't stand alone (a comment outside them, or line breaks that only the
call's parentheses allow), the parentheses stay: `SQL[T]("..."  # c\n)` becomes
`("..."  # c\n)`. A wrapper whose argument is not only string literals
(`SQL[T]("--sql..." + x)`) is left in place, as is a dotted `mod.SQL[T](...)`,
which `check` still checks.

<!-- generated:cli-usage-strip -->
```text
sqlty strip [-h] [--dialect DIALECT] [--schema SCHEMA]
            [--config-file PATH | --isolated] [-c KEY=VALUE]
            [--error RULE] [--warn RULE] [--ignore RULE]
            [--exclude GLOB] [--extend-exclude GLOB] [--force-exclude]
            [--respect-gitignore | --no-respect-gitignore] [-v] [-q]
            [-j N] [--check] [--diff]
            [paths ...]
```
<!-- /generated:cli-usage-strip -->

<!-- generated:cli-options-strip -->
| Option | Description |
|--------|-------------|
| `--check` | Do not modify files; print `Would strip <file>` and exit 1 if any file would change |
| `--diff` | Print a unified diff instead of modifying files; exit 1 if any file would change |
<!-- /generated:cli-options-strip -->

Exit code: see [Exit codes](#exit-codes); 1 with `--check` or `--diff` when files would change.

## `sqlty check`

Lint `--sql` strings and `SQL[T](...)` wrappers. Prints one line per
diagnostic and a summary:

```text
queries.py:2: [narrow-type] SQL[tuple[int, str]] should be SQL[tuple[int, str, str]]
Found 1 error(s)
```

<!-- generated:cli-usage-check -->
```text
sqlty check [-h] [--dialect DIALECT] [--schema SCHEMA]
            [--config-file PATH | --isolated] [-c KEY=VALUE]
            [--error RULE] [--warn RULE] [--ignore RULE]
            [--exclude GLOB] [--extend-exclude GLOB] [--force-exclude]
            [--respect-gitignore | --no-respect-gitignore] [-v] [-q]
            [-j N] [--fix] [--exit-zero] [--exit-zero-on-warning]
            [--error-on-warning] [--statistics] [--explain]
            [--output-format {text,json,github}]
            [paths ...]
```
<!-- /generated:cli-usage-check -->

<!-- generated:cli-options-check -->
| Option | Description |
|--------|-------------|
| `--fix` | Apply the fixable rules (see `sqlty rule`), write the files, then report what remains |
| `--exit-zero` | Exit 0 even when an error-severity diagnostic is reported |
| `--exit-zero-on-warning` | Exit 0 when no diagnostic has severity error (the default); cannot be combined with --error-on-warning |
| `--error-on-warning` | Exit 1 when any diagnostic has severity warn; cannot be combined with --exit-zero or --exit-zero-on-warning |
| `--statistics` | After the summary, print how many queries were typed, partially typed, unparsed and skipped |
| `--explain` | Under a type diagnostic, print the inference rule (id, reference, reasoning) behind each column's type and nullability |
| `--output-format {text,json,github}` | How diagnostics are printed: text (file:line: [rule] message, the default), json (an array of objects with file, line, rule, severity, message, the start and end of what it underlines, and the fix edits) or github (GitHub Actions annotations). Summaries, --statistics and --explain are text only |
<!-- /generated:cli-options-check -->

With `--statistics`, the last line counts the queries of the checked files:

```text
Queries: 1 typed, 1 partially typed, 1 unparsed, 1 skipped (fstring-skipped: 1)
```

A query is typed when every column of its row type is known, and partially
typed when some column's type is not (`SELECT *` with no schema, an unknown
column). Unparsed queries are those the parser rejects, whether or not
[`unparsed-query`](rules.md#unparsed-query) is enabled. Skipped strings are
f-, t- and raw strings, and other `--sql` strings that are not plain literals,
counted by the rule that reports them. Queries that return no rows are not
counted.

Rule severities come from `[tool.sqlty.rules]`, `-c` and `--error`/`--warn`/`--ignore`; see [Rules](rules.md).

Exit code: 1 if any diagnostic has severity `error`; warnings alone exit 0.
As in ty, `--exit-zero` always exits 0, `--error-on-warning` also exits 1 on
a `warn` diagnostic, and `--exit-zero-on-warning` keeps the default; combining
`--error-on-warning` with either of the others is an error (exit 2).

## `sqlty breaking`

Check the queries and schema of two revisions across, as during a rolling
deploy: the queries of `--against` on the schema of `--head` (backward), and the
queries of `--head` on the schema of `--against` (forward). Prints what the other
schema breaks. `--against` is a git revision, a snapshot that `--save` wrote, a
directory of snapshots, or a schema file (see
[Baselines without git history](../guide/breaking-changes.md#baselines-without-git-history)):

```text
app/q.py:2: [unknown-column] column `bio` does not exist in `users` (backward: the queries of origin/main against the schema of the working tree)
Found 1 breaking change(s)
```

<!-- generated:cli-usage-breaking -->
```text
sqlty breaking [-h] [--against BASELINE] [--save PATH] [--head REF]
               [--direction {backward,forward,both}] [-q]
               [--output-format {text,json,github}]
               [paths ...]
```
<!-- /generated:cli-usage-breaking -->

<!-- generated:cli-options-breaking -->
| Option | Description |
|--------|-------------|
| `--against BASELINE` | The baseline: a revision (a branch, tag, SHA or HEAD~1), a snapshot written by --save, a directory of snapshots (each checked), or a schema .sql file or directory (forward only, as it has no queries) |
| `--save PATH` | Write a snapshot of --head (its settings, schema and Python files) to PATH, a baseline for a later --against |
| `--head REF` | The new revision (default: the working tree, uncommitted changes included) |
| `--direction {backward,forward,both}` | backward: queries of --against on the schema of --head (migrate first); forward: queries of --head on the schema of --against (deploy first) |
| `--output-format {text,json,github}` | How findings are printed: text (the default), json (an array of objects with file, line, rule, severity, message, direction and against) or github (GitHub Actions annotations, each at its severity) |
<!-- /generated:cli-options-breaking -->

Exit code: 1 when a change breaks either direction with an `error` diagnostic, 2 for an unknown revision or
outside a git repository. See [Breaking schema changes](../guide/breaking-changes.md).

## `sqlty rule`

Print a rule's documentation, or list every rule with its default severity.

<!-- generated:cli-usage-rule -->
```text
sqlty rule [-h] [name]
```
<!-- /generated:cli-usage-rule -->

## `sqlty skill`

Print an Agent Skill for coding agents (`sqlty` by default), list them, or write
them all into the directory your agent reads skills from. See
[Coding agents](../recipes/agents.md).

```bash
sqlty skill --install .claude/skills
```

<!-- generated:cli-usage-skill -->
```text
sqlty skill [-h] [--install DIR | --list] [name]
```
<!-- /generated:cli-usage-skill -->

<!-- generated:cli-options-skill -->
| Option | Description |
|--------|-------------|
| `name` | Skill to print (default: sqlty) |
| `--install DIR` | Write every skill to DIR/NAME/ instead of printing one |
| `--list` | List the skills and what each is for |
<!-- /generated:cli-options-skill -->

## `sqlty version`

Print `sqlty <version>` and exit. `sqlty --version` does the same.

## Other entry points

| Command | Description |
|---------|-------------|
| `python -m sqlty.magic FILE` | Print the source the [magic codec](../guide/magic-codec.md) produces for `FILE` |

## Environment variables

| Variable | Effect |
|----------|--------|
| `SQLTY_CONFIG_FILE` | Default for `--config-file`; ignored with `--isolated` |
| `SQLTY_NO_MAGIC` | Any non-empty value (even `0`) stops `sqlty.magic` from registering the magic codec on import |
