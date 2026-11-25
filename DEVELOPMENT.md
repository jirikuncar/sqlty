# Development Guide

Setup, commands and architecture of SQLTy. Conventions for code, tests and docs
live in [AGENTS.md](AGENTS.md); review rules in [REVIEW.md](REVIEW.md).

## Prerequisites

- Python 3.14+
- [uv](https://docs.astral.sh/uv/)
- [`rustup`](https://rustup.rs/): `rust-toolchain.toml` pins the Rust version (builds the engine, `sqlty._core`, and runs the cargo hooks)

## Setup

```bash
uv sync --all-groups    # dependencies and dev tools; builds sqlty._core
uv run maturin develop  # rebuild sqlty._core after changing rust/
uv run prek install     # run the hooks on every commit, cargo test on every push
```

sqlty's engine is the Rust extension `sqlty._core`; nothing works without it.

### Hooks

Run every hook before each commit:

```bash
uv run prek run --all-files
```

[prek](https://prek.j178.dev) runs the hooks of `prek.toml` on every commit, and
`cargo test` on every push (`uv run prek run --all-files --hook-stage pre-push`
runs only it). Each runs when the files it depends on change:

| Hook | Runs when these change |
|------|------------------------|
| mypy and ty on `src/` | `src/`, `pyproject.toml` |
| `sqlty rewrite` (in place), `sqlty check`, mypy and ty on `examples/` (except `magic_codec/`) | `examples/`, `src/sqlty/`, `rust/crates/` |
| `cargo fmt --check` | Rust files |
| `cargo clippy -D warnings`, `cargo test` (on push) | `rust/` |
| `tsc`, `oxlint`, `oxfmt` (`npm run check`) | `editors/vscode/` |
| ruff (lint + format), uv-lock | Python files, `pyproject.toml` |
| rumdl | Markdown files |
| Vale | user-facing Markdown (`docs/`, rule pages, top-level guides) |
| `zizmor` | `.github/workflows/` |
| file checks (whitespace, line endings, YAML/TOML/JSON syntax, ...) | any file |

Hooks that edit files run one at a time; the read-only checks run in
parallel. CI's Lint job runs the commit hooks on all files.

## Running tests

| Layer | Location | What it tests |
|-------|----------|---------------|
| **Unit** | `tests/test_*.py` | Invariants over the mdtest corpus, bindings, fuzzing, docs |
| **mdtest** | `tests/mdtest/`, `docs/`, `README.md`, `skills/` | Type inference, rewrite, strip, lint ([format](AGENTS.md#markdown-tests-mdtest)), run by each crate's `tests/mdtest.rs` |
| **E2E** | `tests/e2e/` | CLI in a subprocess, with `inline-snapshot` |
| **Rust** | `rust/crates/*/` | Engine, CLI (`trycmd`), language server |

```bash
uv run pytest                                          # all tests except timing
make test-mdtest                                       # mdtests
uv run pytest tests/ --ignore=tests/e2e --ignore=tests/mdtest  # unit tests
uv run pytest tests/e2e/ -v                            # end-to-end tests
uv run pytest tests/e2e/ --inline-snapshot=update      # accept new e2e output
uv run pytest -k "integer cast"                        # filter by name
uv run pytest -m timing -n 0 -p no:randomly            # the tests that measure wall time
uv run pytest -n 0 -p no:randomly                      # one process, in file order
```

Tests run in parallel (pytest-xdist, `-n auto`) and in random order
(pytest-randomly; a failure prints its `--randomly-seed`). Tests that measure wall
time are marked `timing`: the default run leaves them out, since parallel workers
slow them. Tests marked `pg` run when `SQLTY_PG_DSN` points to a PostgreSQL server;
each worker gets its own database on it (`tests/conftest.py`).

Make targets:

```bash
make test            # all tests, then the timing tests alone
make test-timing     # the tests that measure wall time, alone
make test-unit       # unit tests only
make test-mdtest     # mdtest only
make test-e2e        # e2e only
make update-mdtest   # fill in mdtest assertions with actual values
make test-coverage   # each package's own tests with coverage, and its gate
make check-no-exec-harness  # the CI check that guards the no-exec harness
```

### Rust tests

```bash
cargo test --workspace --manifest-path rust/Cargo.toml
```

CI runs each crate's tests on their own with [cargo-nextest](https://nexte.st/), which
runs test binaries in parallel and reports each failure on its own, then runs the
doctests that nextest skips:

```bash
cargo nextest run -p sqlty-sql --manifest-path rust/Cargo.toml
cargo test --doc -p sqlty-sql --manifest-path rust/Cargo.toml
```

`rust/crates/sqlty-cli/tests/cli/*.md` run the built `sqlty` binary with
[`trycmd`](https://docs.rs/trycmd): each `$ sqlty ...` line in a `console` block,
its output and `? CODE` exit status. `NAME.in/` is the project a case starts
from; with `NAME.out/`, the commands share one copy of it and each file there
must match what they leave; a case that writes files needs one. Write new output into the cases with:

```bash
TRYCMD=overwrite uv run cargo test -p sqlty-cli --test cli --manifest-path rust/Cargo.toml
```

It updates the files already in `.out/`; add new ones by hand, where `...` matches any lines.
Output backslashes read as `/`, so keep `\n` out of what a case prints.

### Packages in CI

Each Rust crate under `rust/crates/` is a package, and so is the Python package
(`src/sqlty` with the extension crate `sqlty-ext`). On a pull request,
[`scripts/ci_changes.py`](scripts/ci_changes.py) picks the packages the changed files
belong to, with every package that depends on one of them, and the Tests workflow runs
only those; a push to `main` runs all of them. A change to a file no package owns runs
everything, unless the script ignores it, as it does `plans/`. The jobs that run the
built extension (free-threaded Python, the database tests, the benchmarks and the
package) are packages of their own, each owning the files it reads: a change to
`tests/test_cli.py` runs none of them. So is `docs`, the Markdown of the guides: a
change to it alone runs the tests that read them and the Markdown tests of `docs/` and
`README.md`, not the Python matrix.
The Docs, VS Code extension and Compatibility workflows have no `paths:` filter: their
first job runs the same script and gates the rest on its `site`, `vscode` or `compat`
output, so one list says what each reads.
The required check is `CI`, which passes when every job the change needed passed.

A package's coverage counts its own tests only: a crate's Rust tests, or for the Python
package pytest and the extension crate's Rust tests. Each must run every line of its
package, except the crates in `scripts/coverage-floors.txt`, whose tests may not miss
more lines than their floor there. `make test-coverage PACKAGES=sqlty-lsp` runs the gate
of one package.

A release publishes the wheels and binaries `main` built from the same inputs (the
`Release` workflow finds them by a hash of those inputs), and builds them only when
`main` never did.

### Tests against the databases

The queries of the Markdown tests also run on real databases, and the result columns
must match sqlty's:

| Test | Database | Compares |
|------|----------|----------|
| `tests/test_pg_describe.py` | PostgreSQL, with `SQLTY_PG_DSN` | names and types; no NULL in a non-null column, on seeded rows |
| `tests/test_server_describe.py` | SQLite, always; MySQL, with `SQLTY_MYSQL_DSN` | names and Python types |
| `tests/test_driver_values.py` | all three | the Python type of the value the driver returns |

```bash
SQLTY_MYSQL_DSN=mysql://root@127.0.0.1:3306/probe uv run --with pymysql pytest tests/test_server_describe.py -rs
```

Each test drops the tables it loads, so point it at a scratch server. A query the
database rejects is skipped, with the reason.

### Fuzzing

`rust/fuzz/tests/properties` holds property tests built with
[`proptest`](https://docs.rs/proptest), each against an oracle of its own:

- `analyzer`: random schemas and queries (joins, aggregates, casts, `COALESCE`,
  `CASE`, `UNION ALL`, `WHERE ... IS NOT NULL`) analyze with one column per select
  item and survive `rewrite` and `strip`. With `SQLTY_PG_DSN`, the server prepares
  each query and must agree on column names and types, and running it over rows
  full of NULLs catches a column typed as non-null that is not.
- `params`: placeholders and call diagnostics against SQLite's binding, and
  `rewrite` with parameter types against Python running the rewritten code.
- `foreign_keys`: `foreign-key-mismatch` in every query shape against a model of
  the rule page.
- `variants`: generated functions run by a model for every input against the
  query variants sqlty reads.
- `suppress`: `-- sqlty: ignore` comments in random `.sql` lines (blank lines,
  comment runs, strings, `\r`, `\r\n`) against a model of which line each covers
  and which diagnostics and `unused-ignore` it leaves.
- `newtypes`: chains of `NewType` arguments across modules and packages, spelled every
  way `typing` allows, bind a query as their supertype does, and stay unknown when the
  module binds a name again; Python resolves each.

```bash
SQLTY_FUZZ_EXAMPLES=2000 SQLTY_PG_DSN=postgresql:///scratch SQLTY_FUZZ_PYTHON=.venv/bin/python \
  cargo test --release --manifest-path rust/fuzz/Cargo.toml --test properties
```

The server's tables are dropped and recreated, so point it at a scratch
database. `SQLTY_FUZZ_PYTHON` must import this checkout's `sqlty` dependencies;
each property prints how many distinct cases it ran (`--nocapture`). In CI, run the `Fuzz`
workflow by hand (Actions, Fuzz, Run workflow); a failure prints the case it
shrank to and the `PROPTEST_RNG_SEED` that replays it. The case belongs in an
mdtest next to the fix.

`rust/fuzz` feeds every playground request (`analyze`, `complete`, `hover`,
`format`) arbitrary source and schema with
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), seeded from the
Markdown tests, with overflow checks and AddressSanitizer:

```bash
cd rust/fuzz
python3 seed.py ../../tests/mdtest corpus/run
cargo +nightly fuzz run -O -a run -- -dict=sql.dict -max_total_time=600
```

A crash leaves its input in `artifacts/run/`; `cargo +nightly fuzz fmt -O -a run
<file>` prints it. The `Fuzz the engine` workflow runs it every night and
uploads the input when it fails.

## Type checking

```bash
make typecheck                 # mypy + ty on src and examples (prek hooks)
make typecheck-examples        # mypy + ty on each examples/ dir (runs in CI)
uv run pytest tests/test_typecheckers.py -v  # mypy, pyright, ty on tests/typechecks/
```

## Formatting

```bash
uv run ruff format .
uv run ruff check --fix .
```

## Benchmarks

The end-to-end and CodSpeed benchmarks need a release build
(`uv run maturin develop --release`). Published wheels use the slower `dist`
profile (fat LTO, one codegen unit; `rust/Cargo.toml`), so `release` numbers can
differ slightly from a shipped wheel.

```bash
make bench-ci         # CI-size project; exits 1 when over benchmarks/baseline.json
make bench-baseline   # re-record the baseline after an intended change
make bench-project    # full-size synthetic project (10k files): CLI wall time and peak RSS
make bench-rust       # Criterion benches in rust/crates/sqlty-bench; extra args via ARGS=
make bench-codspeed   # benchmarks/test_benchmarks.py, as CI's CodSpeed job measures them
make bench-corpus     # sqlty check on pinned real repositories (benchmarks/corpus.py)
```

- `bench-ci` (`benchmarks/bench_project.py`, with `tests/test_performance.py`)
  measures CLI wall time and RSS on a synthetic project, and SQL and Python inference
  against their budgets (AGENTS.md "Check budget"); the `Performance budget` CI job
  runs it. [benchmarks/README.md](benchmarks/README.md) has the metrics and the budget
  rules.
- `bench-rust` covers catalog load (50 to 5000 tables with functions), the
  analyzer per query kind, `check_source`/`rewrite_source` on 1k/4k queries,
  name-index prefix lookups and `complete` at 50, 540 and 5000 tables,
  `query_at` in a module of 4000 queries, and the check of one query by size
  (`one_query`). Use it to find where time goes and to
  compare a branch (`ARGS="--save-baseline main"`, then `ARGS="--baseline main"`).
- `bench-codspeed` runs each query kind of `synth.KINDS` through
  `analyze_select`/`analyze_params`, the schema load, `rewrite_source`,
  `check_source`, `complete_source` at 50, 540 and 5000 tables and in a module
  of 4000 queries.
  CI counts their instructions on every commit and reports the change on the
  pull request.

## Architecture

The engine parses SQL with
[sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs) and Python with
ruff's parser, and is exposed to Python with PyO3, in
[maturin's mixed Python/Rust layout](https://www.maturin.rs/project_layout.html).

```text
src/sqlty/
├── __init__.py          # SQL[T] generic str subclass
├── core.py              # Public API over the Rust engine
├── _core.pyi            # Stub for sqlty._core (rust/crates/sqlty-ext/src/lib.rs)
├── _cli.py              # `sqlty` entry point: runs the Rust CLI, reports crashes
├── _config.py           # Configuration from sqlty.toml, pyproject.toml or Cargo.toml
├── _lint.py             # Lint rules and the Diagnostic type
├── _decode.py           # Magic codec decoder
├── magic.py             # Magic codec registration
├── lexer.py             # Pygments lexer coloring the SQL in Python strings
└── drivers/psycopg.py   # Typed psycopg helpers
rust/
├── Cargo.toml           # Workspace root: shared deps and package metadata
├── fuzz/                # cargo-fuzz target over the playground requests
└── crates/
    ├── sqlty-sql/       # No PyO3, no I/O: analyzer/ (type inference, nullability,
    │   │                # completion, hover), catalog/ (schemas, name index), ddl.rs
    │   └── rules/       # Inference rules (*.inference.toml), compiled by build.rs
    ├── sqlty-rules/     # Lint rule registry, generated from docs/reference/rules/*.md
    ├── sqlty-python/    # Python source: ruff parser, rewriter, checker, fixer, row types
    ├── sqlty-format/    # sqlty format: query strings laid out by sqruff
    ├── sqlty-project/   # A project read from disk: settings, helpers, checks
    ├── sqlty-cli/       # Every sqlty command; main.rs is the standalone sqlty executable
    ├── sqlty-lsp/       # The language server of sqlty server
    ├── sqlty-wasm/      # JSON in, JSON out: @sqlty/wasm (npm/wasm) and the docs playground (make wasm)
    ├── sqlty-ext/       # PyO3 module sqlty._core; cli.rs runs sqlty-cli
    └── sqlty-bench/     # Criterion benchmarks (make bench-rust)
editors/vscode/          # VS Code client of sqlty server; see its README
tests/
├── _mdtest/             # mdtest plugin: parser, the codec's `# decoded:`, --update-mdtest
├── mdtest/              # The specification, as Markdown tests
├── e2e/                 # CLI end-to-end tests and their fixtures
└── type_oracle/         # Python row-type reader, the oracle for the Rust one
```

### Magic codec internals

User documentation: [docs/guide/magic-codec.md](docs/guide/magic-codec.md).

| File | Role |
|------|------|
| `src/sqlty/_decode.py` | Pure decode logic, no side effects; wraps strings with `rewrite_source` |
| `src/sqlty/magic.py` | Codec registration (skipped when `SQLTY_NO_MAGIC` is set) and `python -m sqlty.magic` |
| `src/sqlty_magic.pth` | Startup hook registering a lazy `sqlty` codec lookup (imports `sqlty.magic` on first use, skipped when `SQLTY_NO_MAGIC` is set); shipped at the wheel root via `[tool.maturin] include` (needs maturin >= 1.12.1), not added by editable installs; `tests/check_dist.py` checks built wheels and sdists |
| `tests/mdtest/codec/` | Snapshot tests for `decode()` |
| `tests/test_magic*.py`, `tests/e2e/test_magic_codec_e2e.py` | Integration and end-to-end tests |

How it works:

1. Importing `sqlty.magic` registers a codec search function for `sqlty`
   (and for UTF-8 aliases, which has no effect in practice: CPython decodes
   UTF-8 source without a codec lookup).
2. When Python compiles a file declaring `# coding: sqlty`, it calls
   `decode()` on the raw bytes.
3. `decode()` runs `rewrite_source(..., keep_lines=True)`, which wraps strings
   as `sqlty rewrite` does but joins the imports with `; ` onto an existing
   line (`insert_inline` in `rust/crates/sqlty-python/src/rewriter.rs`), and turns the
   `# coding: sqlty` declaration into `utf-8`. Running a file as a script
   decodes from the end of the coding line and drops the first decoded line
   (`fp_setreadl` in CPython's `Parser/tokenizer/file_tokenizer.c`), so
   imports must not go on a line of their own at the top. A new line is added
   only for a compound statement right after the coding line. On any error it
   returns the source unchanged.
4. Python compiles the transformed text.

Python caches codec lookups, so registration must happen before the first
file declaring `# coding: sqlty` is compiled. Use `sqlty._decode.decode`
directly when you need the transformation without registering the codec.

## Documentation site

The site at <https://sqlty.kuncar.dev/> is built from `docs/` with
[Zensical](https://zensical.org/) (configured in `zensical.toml`). Zensical is
not a project dependency; the Make targets run a pinned version with `uv run --with`, in the
project environment, so sqlty's Pygments lexer (`sqlty.lexer`) colors the SQL of the Python
examples as the playground does. Build the extension first (`maturin develop`).
The version is pinned only in the `Makefile` (`ZENSICAL`); CI runs `make docs`.

```bash
make docs           # strict build into site/ (fails on broken links or anchors)
make docs-serve     # live preview at http://localhost:8000/sqlty/
make generate-docs  # regenerate the tables of configuration.md, cli.md and rules.md
make lint-docs      # rumdl and Vale on the docs
```

- [rumdl](https://github.com/rvben/rumdl) checks formatting and accessibility (heading
  order, image alt text, fenced code languages); `[tool.rumdl]` in `pyproject.toml`
  configures it.
- [Vale](https://vale.sh) checks the prose of the user-facing pages: spelling, American
  English, no Latin abbreviations or em dashes, and inclusive language (the Google and
  alex styles, committed under `.vale/styles`). `.vale.ini` turns off the rules that
  conflict with the house style. Add a project word to
  `.vale/styles/config/vocabularies/SQLTy/accept.txt`.

Add new pages to `nav` in `zensical.toml`. CI builds the site on pull requests
touching the docs and deploys it to GitHub Pages on push to `main`. One-time
setup: in the repository Settings > Pages, set Source to "GitHub Actions";
otherwise the first deploy fails.

## Getting help

- **Contributing**: [CONTRIBUTING.md](CONTRIBUTING.md)
- **Documentation**: <https://sqlty.kuncar.dev/> (sources in [docs/](docs/))
- **Issues**: [GitHub Issues](https://github.com/jirikuncar/sqlty/issues)
