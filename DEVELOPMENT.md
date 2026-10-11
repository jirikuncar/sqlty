# Development Guide

This guide covers development setup, workflows, and contribution guidelines for SQLTy.

## Prerequisites

- Python 3.15+
- [uv](https://docs.astral.sh/uv/) - Fast Python package installer and resolver
- [Rust toolchain](https://rustup.rs/) (builds the engine, `sqlty._core`, and runs the `cargo-fmt` pre-commit hook)

## Setup

### Install Dependencies

```bash
uv sync --all-groups
```

This will install all dependencies and development tools defined in `pyproject.toml`.

### Build the Rust Extension

sqlty's engine is the Rust extension `sqlty._core`; nothing works without it.
`uv sync` builds it; rebuild after changing `rust/`:

```bash
# maturin is installed by `uv sync --all-groups`
uv run maturin develop
```

### Install Pre-commit Hooks

```bash
uv run pre-commit install
```

This ensures code is formatted and checked before each commit.

#### Pre-commit Hooks Configured

- **Standard checks**: AST validation, YAML/TOML/JSON syntax, merge conflicts, trailing whitespace
- **Ruff**: Code formatting and linting with automatic fixes
- **uv-lock**: Keeps the lockfile up to date
- **mypy** and **ty**: Type checking for `src/`
- **sqlty-check** / **sqlty-rewrite**: lint `examples/` and rewrite their `--sql` strings in place (`magic_codec/` excluded)
- **cargo-fmt**: `cargo fmt --check` for the Rust engine (requires `cargo`)

To manually run all pre-commit hooks:

```bash
uv run pre-commit run --all-files
```

## Running Tests

SQLTy has four test layers:

| Layer | Location | What it tests | Runner |
|-------|----------|--------------|--------|
| **Unit** | `tests/test_core_invariants.py`, `tests/generator/`, etc. | Invariants over the mdtest corpus, parser cases, stubs | `pytest` |
| **mdtest** | `tests/mdtest/`, `docs/`, `src/sqlty/rules/`, `README.md` | Type inference, rewrite, strip, lint | `tests/_mdtest` plugin |
| **E2E** | `tests/e2e/` | CLI, subprocess, mypy validation | `pytest` + `inline-snapshot` |
| **Docs** | `README.md`, `docs/**/*.md` | Code examples in user docs | `tests/test_docs.py` (mdtest engine) |

### Make targets

```bash
make test           # Run all tests (unit + mdtest + e2e)
make test-unit      # Unit tests only (no e2e, no mdtest)
make test-mdtest    # Mdtest suite + docs tests
make test-e2e       # End-to-end tests only
make test-coverage  # All tests with coverage report
make update-mdtest  # Update mdtest assertions with actual values
make bench          # sqlty.core benchmarks
```

### Using pytest directly

```bash
# All tests
uv run pytest

# Specific test layer
uv run pytest tests/mdtest/ docs/ src/sqlty/rules/ README.md -v
uv run pytest tests/ --ignore=tests/e2e --ignore=tests/mdtest
uv run pytest tests/e2e/ -v

# Filter by test name
uv run pytest -k "integer cast"
uv run pytest tests/mdtest/type_inference/basic_types.md -v
```

### Markdown Tests (mdtest)

Format, assertions and workflow: see [AGENTS.md](AGENTS.md#markdown-tests-mdtest).

**Test directory layout:**

```
tests/mdtest/
  type_inference/   # basic types, datetime, uuid, dialects, schema inference
  postgres/         # SELECT, RETURNING, CTE, JSON, aggregates, star expansion, advanced
  rewrite/          # basic, edge cases, import injection, idempotency, multiline
  strip/            # basic, round-trip, idempotency, import removal
  check/            # star expansion, type annotation, user types, lint rule config
  codec/            # basic decode, auto imports, runtime execution
```

### E2E Tests

E2e tests in `tests/e2e/` use subprocess to run the sqlty CLI (no internal imports). They use `inline-snapshot` for result verification and include mypy validation.

```bash
# Run e2e tests
uv run pytest tests/e2e/ -v

# Update e2e snapshots when output changes intentionally
uv run pytest tests/e2e/ --inline-snapshot=update
```

## Development Workflow

### Type Checking

```bash
make typecheck
```

Or directly:

```bash
uv run mypy src/sqlty
uv run ty check src/sqlty
```

### Code Formatting

```bash
uv run ruff format .
uv run ruff check --fix .
```

## Architecture

### Core modules

| Module | Purpose |
|--------|---------|
| `core.py` | Public API over the Rust `_core`, plus user-defined row type checks |
| `_core.pyi` | Type stubs for the Rust extension module |
| `_cli.py` | ruff-style CLI: `rewrite`, `strip`, `check`, `version` |
| `_lint.py` | Lint rules and their severity (error/warn/ignore) |
| `_type_extractor.py` | Row types (TypedDict, dataclass, ...) for structural checks |
| `_decode.py` | Magic codec decoder (bytes -> string transformation) |
| `magic.py` | Codec registration for Python import system |

### Rust extension (`rust/`)

The engine parses SQL with
[sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs) and is exposed
to Python with PyO3.
It follows [maturin's recommended mixed Python/Rust project layout](https://www.maturin.rs/project_layout.html).

```
rust/
├── Cargo.toml        # Crate config (pyo3, sqlparser, regex)
└── src/
    ├── lib.rs        # PyO3 module entry: the sqlty._core functions
    ├── analyzer.rs   # SQL parsing, type inference, schemas, SELECT * expansion
    ├── rewriter.rs   # Python source rewrite/strip, import-style
    ├── checker.rs    # Lint checks (stale-rewrite, stale-type, SELECT *, ...)
    └── fixer.rs      # check --fix
```

Run Rust unit tests independently:

```bash
cargo test --manifest-path rust/Cargo.toml
```

### Magic codec internals

User documentation: [docs/guide/magic-codec.md](docs/guide/magic-codec.md).

| File | Role |
|------|------|
| `src/sqlty/_decode.py` | Pure decode logic, no side effects; wraps strings with `rewrite_source` |
| `src/sqlty/magic.py` | Codec registration (skipped when `SQLTY_NO_MAGIC` is set) and `python -m sqlty.magic` |
| `sqlty_magic.pth` | `import sqlty.magic` hook for site-packages; not included in the wheel |
| `tests/mdtest/codec/` | Snapshot tests for `decode()` |
| `tests/test_magic*.py`, `tests/e2e/test_magic_codec_e2e.py` | Integration and end-to-end tests |

How it works:

1. Importing `sqlty.magic` registers a codec search function for `sqlty`
   (and for UTF-8 aliases, which has no effect in practice: CPython decodes
   UTF-8 source without a codec lookup).
2. When Python compiles a file declaring `# coding: sqlty`, it calls
   `decode()` on the raw bytes.
3. `decode()` runs `rewrite_source`, as `sqlty rewrite` does, and turns the
   `# coding: sqlty` declaration into `utf-8`. On any error it returns the
   source unchanged.
4. Python compiles the transformed text.

Python caches codec lookups, so registration must happen before the first
file declaring `# coding: sqlty` is compiled. Use `sqlty._decode.decode`
directly when you need the transformation without registering the codec.

### Test modules

| Module | Purpose |
|--------|---------|
| `tests/_mdtest/` | mdtest plugin: parser, runner over `sqlty.core`, `--update-mdtest` |
| `tests/e2e/conftest.py` | E2E fixtures, CLI runner helpers |

## Documentation Site

The site at <https://jirikuncar.github.io/sqlty/> is built from `docs/` with
[Zensical](https://zensical.org/) (configured in `zensical.toml`). Zensical is
not a project dependency; the Make targets run a pinned version via `uvx`.
The version is pinned only in the `Makefile` (`ZENSICAL`); CI runs `make docs`.

```bash
make docs        # strict build into site/ (fails on broken links or anchors)
make docs-serve  # live preview at http://localhost:8000/sqlty/
```

Add new pages to `nav` in `zensical.toml`. CI builds the site on pull requests
touching the docs and deploys it to GitHub Pages on push to `main`. One-time
setup: in the repository Settings > Pages, set Source to "GitHub Actions";
otherwise the first deploy fails.

## Contributing

### Development Process

1. Fork and clone the repository
2. Create a branch for your feature/fix
3. Make changes with tests
4. Run `make test` and `make typecheck`
5. Commit with a clear message
6. Push and create a pull request

### Code Style

- Follow PEP 8 and PEP 484 (type hints)
- Use Ruff for formatting and linting
- Add type hints to all functions

### Testing Guidelines

- **New type inference behavior**: Add mdtest cases in `tests/mdtest/`
- **New lint rules**: Add mdtest cases in `tests/mdtest/check/`
- **CLI changes**: Add e2e tests in `tests/e2e/`
- **Internal changes**: Add unit tests alongside existing test files
- Use `make update-mdtest` to bootstrap expected values

### Pull Request Guidelines

- Write clear PR descriptions
- Reference related issues
- Ensure CI passes
- Request review from maintainers

## Getting Help

- **Documentation**: <https://jirikuncar.github.io/sqlty/> (sources in [docs/](docs/))
- **Issues**: Report bugs or request features on [GitHub Issues](https://github.com/jirikuncar/sqlty/issues)

## License

MIT License - see [LICENSE](LICENSE) file for details.
