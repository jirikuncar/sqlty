# VS Code extension

> **Status:** stages 1 to 3 implemented. `Diagnostic` has `start`,
> `end` and `fix` (with the imports a fix needs), `--output-format json` prints them, and
> mdtest asserts `# error[9:19]:`. The bridge is `sqlty._lsp_bridge.Workspace`; the
> [spike](#spike-results) picks [option B](#b-native-rust-server-preferred), a native
> Rust server: `rust/crates/sqlty-lsp`, run by `sqlty server`, and its client is
> `editors/vscode/`. Next: breaking.

## Goal

A VS Code extension, in this repository, that:

1. reports `sqlty check` diagnostics as you type, underlined at the query, the row type or
   the SQL name they are about;
2. offers each fixable diagnostic as a quick fix, and `source.fixAll.sqlty` for the file
   (`sqlty check --fix`), so a stale `SQL[T]` is rewritten on save;
3. when enabled, reports what a schema migration breaks (`sqlty breaking`) against a
   configured baseline.

## What exists

| Need | Today | Gap |
|------|-------|-----|
| Diagnostics | `core.check_source` over `ParsedSource`, `HelperIndex`, a loaded `Schema` | `Diagnostic` has a `line` only (`_lint.py`) |
| Fixes | Rust `Diagnostic.fix: Vec<Edit>` with byte ranges (`sqlty-python/src/checker.rs`) | `DiagnosticOut` (`sqlty-ext/src/lib.rs`) drops them; Python sees whole-file `fix_source` only |
| Positions in SQL | `Group.start`/`end`, `Call.span` (`scan.rs`); sqlparser 0.63 implements `Spanned` | `query_diagnostics` returns `(rule, message)`, no span |
| Breaking | `core.breaking_source(source, schema_sql=, against_sql=)` | the baseline comes from a `git worktree` or snapshot (`_breaking.py`): too slow per keystroke |

## Prior art

- [ruff-vscode](https://github.com/astral-sh/ruff-vscode) runs the native `ruff server`. It looks for
  `ruff` in the selected interpreter's environment, then on `PATH`, then uses the bundled one,
  and in an untrusted workspace uses only the bundled one. Fixes are quick fixes and
  `source.fixAll.ruff`.
- `ruff_server` and `ty_server` are built on
  [`lsp-server`](https://crates.io/crates/lsp-server) and
  [`lsp-types`](https://crates.io/crates/lsp-types), the synchronous transport of rust-analyzer.
- [vscode-python-tools-extension-template](https://github.com/microsoft/vscode-python-tools-extension-template):
  a TypeScript client and a pygls server (`bundled/tool/lsp_server.py`), with
  `interpreter`, `path` and `importStrategy` settings.
- sqlparser added `Spanned` in [0.53](https://apache.googlesource.com/datafusion-sqlparser-rs/+/HEAD/changelog/0.53.0.md);
  [coverage is partial](https://docs.rs/sqlparser/0.62.0/sqlparser/ast/trait.Spanned.html)
  (`Expr` among the partial types), so a whole-string range stays the fallback.

## Approaches

Both options use the same client and the same `sqlty server` command; they differ in what
runs the protocol.

### A. Python server (pygls)

`src/sqlty/_lsp.py` on [pygls](https://pypi.org/project/pygls/) 2.x, behind the extra
`sqlty[lsp]`. It calls `check_source`, `fix_source` and `breaking_source` as the CLI does.

- Small: no new Rust; the `_check.py` passes and `_type_extractor/` run unchanged.
- Protocol handling, debouncing and text sync run in Python. Checks run in worker threads,
  and the Rust calls `detach` from the GIL.
- A new runtime dependency (pygls, lsprotocol) for anyone who installs the extra.

### B. Native Rust server (preferred)

A `sqlty-lsp` crate on `lsp-server` and `lsp-types`, linked into `sqlty-ext`.
`sqlty server` calls `_core.serve()`, so the server ships in the existing wheel
and needs no second binary.

- Rust owns the protocol: stdio transport, open documents and their line index, UTF-16
  positions, debouncing, cancellation, the `Schema` cache, code actions and fix-all.
- The Rust passes (`checker.rs`, `fixer.rs`, the analyzer) are called directly, with no
  conversion through Python.
- The Python passes (`_check.py` row types, `_type_extractor/`, `_variants.py`,
  `_psycopg.py`; about 13.7k lines of `src/sqlty`) stay Python. The server calls them
  through one bridge, `sqlty._lsp_bridge.check(path, text) -> list[Diagnostic]`, taking the
  GIL on a worker thread. Each pass ported to Rust later drops out of the bridge.
- Config (`_config.py`) is read once through the bridge and re-read when `pyproject.toml`
  changes.
- Follows ruff and ty, so their patterns for scheduling and testing apply.

B costs more up front but puts the latency-sensitive part in Rust and keeps the porting path
open; A is the fallback if the bridge's GIL contention proves a problem in the spike.

## Client

`editors/vscode/`: `package.json`, `src/extension.ts` on `vscode-languageclient`, bundled
with esbuild, packaged with `@vscode/vsce`.

| Setting | Default | Meaning |
|---------|---------|---------|
| `sqlty.path` | `[]` | `sqlty` executables to try first |
| `sqlty.interpreter` | selected interpreter | where `sqlty` is looked up |
| `sqlty.importStrategy` | `fromEnvironment` | `fromEnvironment` or `useBundled` |
| `sqlty.breaking.against` | unset | baseline: a revision or a `--save` snapshot; unset disables breaking |
| `sqlty.breaking.direction` | `both` | as `--direction` |

Lookup order follows ruff: the interpreter's environment, `PATH`, the bundled wheel. The
project's own pin wins, so the editor agrees with CI. In an untrusted workspace only the
bundled server runs.

## Features

### Diagnostics

- `Diagnostic` gains `start` and `end` (byte offsets; the line stays). The Rust checker
  sets them from the call or group span; Python passes from the annotation's AST node.
- The server converts offsets to LSP positions (UTF-16) and sets `source: "sqlty"`,
  `code` to the rule and `codeDescription` to its docs page.
- `# sqlty: ignore[...]` is offered as a quick fix for every rule.

### Positions inside SQL

- `query_diagnostics` returns the span of the name it reports (`unknown-column`,
  `unknown-table`, ...), from sqlparser `Spanned`; `None` falls back to the whole string.
- A SQL offset maps back to the source through the string's escapes: a map from
  decoded to source offsets, built next to `decode_escapes`. Raw strings map one to one.

### Fixes

- `DiagnosticOut` carries `fix` as `(start, end, replacement)` edits: one quick fix per
  diagnostic (`stale-type`, `inline-sql-comment`, `select-star`, `mysql-sql-marker`, ...).
- `source.fixAll.sqlty` runs `fix_source`, which also applies the Python-only fixes
  (`json-types`, `param-types`). The returned text is diffed into minimal `TextEdit`s.

### Breaking (opt-in)

- The baseline resolves once: `git rev-parse` of `sqlty.breaking.against`, then its schema
  and settings (`git show` where the schema is files, a worktree otherwise). Cached by SHA,
  re-resolved when the ref moves.
- Forward (head queries on the baseline schema) runs with each check:
  `breaking_source(text, schema_sql=head, against_sql=baseline)`, as source
  `sqlty breaking (forward)`.
- Backward (baseline queries on the head schema) runs when a schema file is saved, over the
  baseline's files, and is published under those paths. Findings in files that have since
  moved or changed are listed in the Problems panel only.

## Stages

1. **Positions and fixes in the core**: `start`/`end` on `Diagnostic`, `fix` through
   `DiagnosticOut`, `_core.pyi` updated. Useful without an editor: `--output-format json`
   gains columns and fixes.
2. **Spike**: B's bridge on a large project (`benchmarks/bench_project.py`); measure check
   latency per keystroke with and without GIL contention. Decide A or B.
   [Results](#spike-results): B.
3. **Server and client**: diagnostics, quick fixes, fix-all, settings, discovery.
   The server is done: full text sync, UTF-8 positions when the client offers them, one
   check at a time on a worker that owns a `Workspace` per root, `pyproject.toml` and
   `.sql` changes reload the project. The client finds `sqlty server` in `sqlty.path`, the
   interpreter, then `PATH`, and restarts when they change; until stage 6 bundles a wheel
   it does not run in untrusted workspaces. Left for later: the `# sqlty: ignore[...]` quick
   fix, which must not land inside a string, and `redundant-optional` across files.
4. **Breaking**: forward, then backward.
5. **SQL spans**: names inside the query string.
6. **Release**: Marketplace and [Open VSX](https://open-vsx.org) (Cursor and VSCodium),
   `docs/recipes/vscode.md`.

## Spike results

`benchmarks/bench_lsp_bridge.py` loads the project of `bench_project.py` (8,000 files,
2,400 queries, 400 tables) into `Workspace` and times a check per keystroke. Release build,
Apple M-series, ms:

| Measure | Median | p95 | Max |
|---------|--------|-----|-----|
| Load (once per server start) | 840 to 3,960 (warm or cold disk) | | |
| Check, a generated module (3 queries) | 0.5 | 1.0 | 41 |
| Check, 50 modules in one file (92 queries) | 14 | 15 | 16 |
| Check after an edit to the helpers module | 164 | 166 | 181 |
| Lag of a thread sleeping 1 ms during checks | 0.3 | 3.8 | 10 |
| Two large checks, one after the other | 36 | 40 | |
| Two large checks at once | 35 | 39 | |

- A check costs about 0.2 ms per query, nearly all of it the Python passes. Checks
  serialize on the GIL, in A and B alike: the server runs one at a time, newest buffer
  first, and drops a check whose buffer has changed.
- The Python passes hold the GIL for up to about 10 ms. Only A's protocol loop waits on it;
  B's Rust thread never takes it, so cancellation and text sync stay responsive.
- An edit to a module that defines helpers rebuilds the helper index (`Project._index`),
  160 ms on this project. Rebuilding only when the helper names change is a later gain.

Contention is not a problem for the bridge, so B goes ahead.

## Tests

- mdtest: `# error:` gains an optional range, `# error[12:18]: [rule] "..."`, asserted
  against `start`/`end` (`tests/_mdtest/`). Escaped and raw strings, multi-line queries.
- `# fixed:` already covers fix-all; per-diagnostic fixes are checked to compose to it in
  `tests/test_core_invariants.py`.
- Server: B with `lsp_server::Connection::memory()` in `cargo test`; end to end through
  `sqlty server` in `tests/test_lsp.py`, with a small JSON-RPC client:
  [pytest-lsp](https://lsp-devtools.readthedocs.io/en/latest/) 1.0.1 breaks the Markdown
  tests (its `pytest_runtest_call` wrapper does not yield for items without `funcargs`).
- Client: `@vscode/test-electron` smoke test, in CI only.
- Hooks: a TypeScript lint and typecheck hook in `prek.toml` for `editors/vscode/`.

## Open questions

- Should a check of an unsaved buffer resolve helpers and row types of other unsaved
  buffers, or the files on disk only?
- Publisher name and whether the extension version follows the `sqlty` version.
- Notebooks: queries in cells (LSP 3.17 notebook sync) are out of scope for stage 3.
