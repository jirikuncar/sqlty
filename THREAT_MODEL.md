# Threat model

What sqlty must protect, where untrusted input enters, and how a finding is
rated. [SECURITY.md](SECURITY.md) says how to report one and how reports are
handled.

## What sqlty is

sqlty reads Python source, SQL query strings and SQL schema files, infers row
and parameter types, and reports or rewrites them. It never connects to a
database and never runs the queries or the code it analyzes. The engine is Rust
(`rust/crates/`) on sqlparser-rs and the ruff Python parser, shipped as:

- the `sqlty` Python package: the PyO3 extension `sqlty._core`, the CLI, the
  `# coding: sqlty` codec with its `sqlty_magic.pth` startup hook, and the
  runtime library (`SQL`, `SQLTemplate`, `Bound`, `sqlty.drivers.psycopg`);
- the native `sqlty` executable on npm and Homebrew (`sqlty-cli`);
- the language server (`sqlty server`, `sqlty-lsp`);
- WebAssembly in `@sqlty/wasm`, the docs playground and the VS Code extension.

## Trust boundaries

Trusted: the user running sqlty, its command line, environment and
configuration (`sqlty.toml`, `[tool.sqlty]`). Configuration is trusted as a
Makefile is: `format-command` runs the command it names by design.

Untrusted:

- Python source, and the SQL strings, t-strings and comments in it. Developers
  run `sqlty check` or open an editor on code they did not write: a pull
  request, a dependency, a cloned repository.
- SQL schema and migration files named by `schema` and `queries`.
- Everything the playground reads from its URL: the session in the hash, the
  `?steps=` deck and the engine version.
- Workspace files opened in VS Code while the workspace is untrusted
  (Restricted Mode).
- At run time in an application, the parameter values passed to `SQL.bind`
  and the psycopg helpers.

## Assets and what would break them

| Component | Paths | A finding is |
|-----------|-------|--------------|
| No execution while analyzing | `src/sqlty/core.py`, `src/sqlty/magic.py`, `rust/crates/sqlty-python/` | `check`, `server`, the codec's analysis or the core API importing, `exec`-ing or `eval`-ing what a source names (`tests/_no_exec.py` guards this) |
| Generated code | `rewriter.rs`, `fixer.rs`, `edit.rs`, `unparse.rs`, `templates.rs`, `types/`, `rows/` in `sqlty-python`; `src/sqlty/_decode.py` | SQL or schema text that makes `rewrite`, `--fix` or the codec emit Python that runs attacker-chosen code, or changes what the module does beyond wrappers and imports |
| Codec opt-in | `src/sqlty_magic.pth`, `src/sqlty/magic.py` | the codec acting on a file without `# coding: sqlty`, or with `SQLTY_NO_MAGIC` set |
| Parsers and inference | `sqlty-sql`, `sqlty-format`, `parse.rs`, `scan.rs`, `queries.rs`, `compose.rs`, `variants.rs` | a crash, a stack overflow, super-linear time or unbounded memory from a small input |
| Files read and written | `sqlty-project`, `config.rs`, `src/sqlty/_config.py` | `rewrite`, `strip`, `format` or `--fix` writing outside the given paths or through a symlink; reading a file sqlty was not configured to read |
| `sqlty breaking` | `src/sqlty/_breaking.py`, `breaking.rs` | a ref, path or file name becoming a git option or a pathspec other than a literal path |
| Language server | `sqlty-lsp`, `sqlty-cli/src/server.rs`, `sqlty-ext/src/server.rs` | a document crashing or stalling the server; a URI that reads or writes outside the workspace |
| Playground | `docs/assets/playground/`, `sqlty-wasm` | script injection from a share link or a deck; a link loading a deck, engine or model from another origin |
| VS Code extension | `editors/vscode/src/` | running a binary or interpreter from the workspace (`sqlty.path`, `sqlty.interpreter`) before the workspace is trusted |
| Runtime library | `src/sqlty/__init__.py`, `src/sqlty/drivers/` | a parameter value reaching SQL text instead of being bound; `row_factory` constructing anything other than the declared row type |
| Releases | `.github/workflows/pypi.yml`, `vscode.yml` | a pull request or workflow input that reaches publishing credentials, or a published artifact not built from the tagged commit |

Parsing limits already in place: Python files over 256 KiB or nested deeper
than `MAX_DEPTH` are skipped (`parse.rs`), SQL nesting is capped by
sqlparser's recursion limit, catalogs by `MAX_COLUMNS` and the loader budgets,
and formatting by `MAX_TOKENS`. A panic during `sqlty check` is reported as
`internal-error`; elsewhere it is a crash.

## Severity

- **Critical**: code execution while analyzing untrusted input (`check`,
  `server`, the core API, the playground, VS Code in Restricted Mode);
  `rewrite`, `--fix` or the codec emitting Python that runs attacker-chosen
  code; a parameter value reaching SQL text through the runtime library;
  publishing credentials reachable from a pull request.
- **High**: writing or deleting files outside the given paths; script
  injection in the playground from a link; the codec transforming a file that
  did not opt in; git option or pathspec injection in `breaking`.
- **Medium**: reading a file sqlty was not configured to read; memory
  corruption from `unsafe` code or a dependency, even when it only crashes.
- **Low**: a crash, a stall, super-linear time or unbounded memory from a small
  input. These are bugs, reported as public issues.

## Out of scope

- Wrong or imprecise types, missed or spurious diagnostics, and rewrites that
  are valid Python but type the query wrongly.
- Anything that needs hostile configuration: `format-command`, `extend`,
  `sqlty.path` in a trusted workspace.
- Bugs in sqlparser-rs, ruff, sqruff, psycopg or other dependencies, unless
  sqlty uses them unsafely. Report those upstream.
- Cost that grows linearly with the input.
- `tests/`, `benchmarks/`, `scripts/`, `plans/`, `examples/`, `docs/` other
  than the playground, and `rust/crates/sqlty-bench/`.
