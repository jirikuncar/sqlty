# Review instructions

Agents self-review a branch with the [`review` skill](.claude/skills/review/SKILL.md),
which applies this file.

## What Important means here

Reserve Important for findings that break behavior users rely on:

- Wrong inferred types, or `sqlty check` missing or inventing a diagnostic.
- `rewrite`/`strip` changing string content, or breaking
  `strip(rewrite(x)) == x` (see the import-line exception in AGENTS.md).
- Non-idempotent `rewrite`, `strip` or `check --fix`.
- Rust engine panics reachable from Python input.
- The magic codec or `sqlty_magic.pth` affecting files without `# coding: sqlty`,
  or failing to return the source unchanged on error.
- `src/sqlty/_core.pyi` out of sync with `rust/crates/sqlty-ext/src/lib.rs`.
- Wheel or sdist missing `sqlty_magic.pth`.
- Hosts disagreeing: CLI, `sqlty server`, the wasm playground and VS Code web
  giving different diagnostics, fixes or types for the same file and settings.
- SQL inference slower than a database round trip, Python inference slower than
  `ruff check`, or cost that grows faster than linearly with any input size (AGENTS.md
  "Check budget").
- A guard weakened: coverage gate, no-exec harness, clippy lints, an mdtest
  assertion removed or loosened, a test skipped, xfailed or deleted to get green.
- Committed build output (`rust/target*`, `.so`, wasm, `*.snap.new`) or secrets.

Style, naming and refactoring suggestions are Nit at most.

## Always check

- New features and behavior changes have an mdtest in `tests/mdtest/`;
  user-visible behavior is also documented in `docs/`.
- New lint rules have a page in `docs/reference/rules/` with its front matter (the only place
  its name, severity, group and summary are written).
- A new lint rule is on by default, per [Lint rule defaults](CONTRIBUTING.md#lint-rule-defaults);
  one that ships off names on its rule page the rule or workflow it conflicts with.
- Test SQL uses `--sql\n`, never `--sql ` followed by a space.
- Docstrings and `--help` text explain usage, with a copy-paste example.
- Pull requests made with an AI agent disclose it, as required by
  CONTRIBUTING.md.

## Correctness

- **Every input is reachable.** Ask what the change does with: unparsable or
  partial source, empty strings, surrogates and non-UTF-8, CRLF, tabs and
  non-ASCII before a span (columns are characters, not bytes), nested and
  triple-quoted f-/t-strings, deep nesting, huge files, `SQL[...]` behind
  aliases and re-exports, every dialect the code path claims to support.
- **No panics in the engine.** No `unwrap`, `expect`, indexing or slicing that
  input can make fail, outside tests. The CLI reports any panic as a crash; elsewhere
  only `check::module` catches one, as `internal-error`. In `sqlty server` a panic in
  completion or hover ends the server and one in its worker stops checking; wasm32
  (`panic=abort`) traps, so the playground and VS Code web restart the engine. Prefer
  `Option`/`Result` combinators.
- **Parity.** A behavior change lands in every host that runs it (`sqlty-cli`,
  `sqlty-project`, `sqlty-lsp`, `sqlty-wasm`, `editors/vscode`, the playground
  settings it reads). Differences on purpose are listed in the plan, not silent.
- **CPython parity.** `unparse` and anything that rebuilds Python text matches
  `ast.unparse`/the interpreter, including `LEGACY_FIELD_TEXT` for t-/f-string
  field text across 3.14 and 3.15. Test against the interpreter, not a hand-written
  expectation.
- **Order and determinism.** Output must not depend on set/hash order, which
  module was checked first, thread count or test order (pytest-randomly, xdist).
- **Concurrency.** rayon pools, locks and forks: no deadlock after `fork`, no
  default 2 MiB stacks for the parser, nothing that waits on a check from a
  completion request.
- **Faster than the database.** SQL inference stays under a database round trip (100 µs
  per query) and Python inference under `ruff check` of the file (AGENTS.md "Check
  budget"); both scale at most linearly: double the input that the change touches (files,
  queries, tables, columns, statements, nesting, line length) up to 32x and compare times.
  Look for `contains` on a `Vec`, a clone or rescan per item, and work repeated per query
  that could run once per file or schema.
- **Spans and fixes.** A fix edits only what its rule names, keeps the file
  parsable, and a second `--fix` changes nothing.
- **Types match the database.** A new or changed inference rule cites the vendor
  documentation of the targeted version and is checked against a real engine when
  one is available (PostgreSQL, MySQL, SQLite, DuckDB). PostgreSQL resolves mixed
  types by category; `int` is not `Literal[1]`; booleans are not numbers there.

## Tests

- A bug fix comes with a test that fails without it. Name the failing input.
- mdtest for behavior, unit tests only for what mdtest cannot express. No duplicate
  sections (`tests/test_duplicates.py`); reuse fixtures and parametrize instead of
  copying setup.
- 100% coverage: Python `fail_under = 100`, Rust gated on an empty
  `cargo llvm-cov report --show-missing-lines` list (nightly). Cover lines with
  real tests, not `#[coverage(off)]` (test modules and Debug impls only) or Hypothesis luck (the
  coverage profile runs explicit examples only). Restructure dead branches: a `?`
  alone on a line or an unreachable `Err` arm counts as missed.
- No `pytest.importorskip`, no skips or xfails beyond what AGENTS.md
  "Writing tests" allows, no wall-time assertions outside
  `-m timing`. Hypothesis strategies are built once (`functools.cache`), not per
  example.
- Snapshots (`inline-snapshot`, `make update-mdtest`) are read, not trusted: a
  regenerated expectation must be the right answer.

## Best practices

- **Less code.** Prefer deleting code, reusing what the workspace already has (ruff
  lexer and parser, sqlparser, `ignore`, existing helpers and constants) and fixing
  the cause upstream over a workaround. A workaround names the upstream issue and
  when to drop it.
- **Structure over strings.** Match on parsed structure or typed data, not on
  formatted text or error messages; exhaustive `match` with no catch-all hiding new
  variants.
- **Comments say why**, briefly. No comments that restate the code, no history
  ("previously", "now"), no commented-out code.
- **Rust**: no I/O in `sqlty-sql`/`sqlty-python` (host traits instead), no `clone`
  to satisfy the borrow checker where a borrow works, no allocation in hot lookups
  (`rule_dispatch_does_not_allocate`), completion stays sub-linear in the number of
  tables (see AGENTS.md "Completion budget").
- **Performance claims** and changes on a check path come with numbers from
  `make bench-ci`, `bench-rust` or a script in the PR, against the merge base, same machine, and identical output.
- **Public surface**: new API in `sqlty.core.__all__` and
  `docs/reference/python-api.md`; new config keys in `KEY_DOCS` (`scripts/generate_docs.py`) and the playground
  and VS Code settings; generated docs current (`make generate-docs`).

## Do not report

- Anything CI enforces: ruff, mypy, ty, rustfmt, clippy.
- `uv.lock`, `rust/Cargo.lock` and expected values filled in by
  `make update-mdtest` or `--inline-snapshot=update`, unless they contradict the change.
- `plans/2026-02-13-future.md`.
- Speculation you did not try to reproduce. Run it, or mark it Unverified.

## Cap the nits

Report at most five Nits per review and count the rest in the summary. After the
first review of a pull request, report Important findings only.
