# Development Guide

This guide covers development setup, workflows, and contribution guidelines for SQLTy.

## Prerequisites

- Python 3.12+
- [uv](https://docs.astral.sh/uv/) - Fast Python package installer and resolver
- [Rust toolchain](https://rustup.rs/) (optional — for compiling the native extension)

## Setup

### Install Dependencies

```bash
uv sync --all-groups
```

This will install all dependencies and development tools defined in `pyproject.toml`.

### Build the Rust Extension (optional)

SQLTy includes an optional Rust extension (`sqlty._core`) for faster type inference.
When not compiled, the pure-Python fallback (`sqlty._core_py`) is used automatically.

```bash
# Install maturin
pip install maturin

# Build and install the Rust extension into your venv
maturin develop
```

To verify the Rust extension is loaded:

```bash
python -c "from sqlty._core import infer_type; print('Rust backend active')"
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
- **mypy**: Type checking for the src directory
- **sqlty-examples**: Verify example files

To manually run all pre-commit hooks:

```bash
uv run pre-commit run --all-files
```

## Running Tests

SQLTy has three test layers:

| Layer | Location | What it tests | Runner |
|-------|----------|--------------|--------|
| **Unit** | `tests/sql_analyzer/`, `tests/generator/`, etc. | Internal modules | `pytest` |
| **mdtest** | `tests/mdtest/*.md` | Type inference, rewrite, strip, lint | Markdown snapshot engine |
| **E2E** | `tests/e2e/` | CLI, subprocess, mypy validation | `pytest` + `inline-snapshot` |

### Make targets

```bash
make test           # Run all tests (unit + mdtest + e2e)
make test-unit      # Unit tests only (no e2e, no mdtest)
make test-mdtest    # Mdtest suite only
make test-e2e       # End-to-end tests only
make test-coverage  # All tests with coverage report
make update-mdtest  # Update mdtest assertions with actual values
```

### Using pytest directly

```bash
# All tests
uv run pytest

# Specific test layer
uv run pytest tests/mdtest/ -v
uv run pytest tests/ --ignore=tests/e2e --ignore=tests/mdtest
uv run pytest tests/e2e/ -v

# Filter by test name
uv run pytest -k "integer cast"
uv run pytest tests/mdtest/type_inference/basic_types.md -v
```

### Markdown Tests (mdtest)

SQLTy uses a snapshot-based markdown test framework inspired by [ty's mdtest](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md). Each `.md` file in `tests/mdtest/` is a test suite; each heading is a test case.

**Assertion types:**

| Prefix | What it tests |
|--------|-------------|
| `# inferred:` | Type inference via `SQLAnalyzer.generate_return_type()` |
| `# rewritten:` | Source rewriting via `SQLRewriter.rewrite()` |
| `# stripped:` | Source stripping via `SQLRewriter.strip()` |
| `# expanded:` | Star expansion via `SQLAnalyzer.expand_star()` |
| `# error:` | Lint diagnostics via `Checker.check_query()` |
| `# decoded:` | Magic codec via `decode()` |

**Writing a new test:** Add a heading + code blocks to the appropriate `.md` file. Use `make update-mdtest` to fill in expected values automatically.

**Test directory layout:**

```
tests/mdtest/
  type_inference/   # basic types, datetime, uuid, dialects, schema inference
  postgres/         # SELECT, RETURNING, CTE, JSON, aggregates, star expansion, advanced
  type_codec/       # TypeCodec protocol: tuple codec, custom codecs
  rewrite/          # basic, edge cases, import injection, idempotency, multiline
  strip/            # basic, round-trip, idempotency, import removal
  check/            # star expansion, type annotation, lint rule config
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
| `core.py` | Public API: try Rust `_core`, fall back to Python `_core_py` |
| `_core_py.py` | Pure-Python computation functions (FFI boundary fallback) |
| `_core.pyi` | Type stubs for the Rust extension module |
| `_cli.py` | ruff-style CLI: `rewrite`, `strip`, `check`, `version` |
| `_sql_analyzer.py` | SQL parsing, type inference, star expansion (sqlglot-based) |
| `_type_codec.py` | TypeCodec protocol + TupleCodec default |
| `_rewriter.py` | Source-level rewrite/strip (`"--sql ..."` <-> `SQL[T](...)`) |
| `_lint.py` | 7 lint rules with ty-style severity (error/warn/ignore) |
| `_checker.py` | Column count/type/name validation, SELECT * detection |
| `_decode.py` | Magic codec decoder (bytes -> string transformation) |
| `magic.py` | Codec registration for Python import system |

### Rust extension (`rust/`)

The Rust extension reimplements the core computation functions using
[sqlparser-rs](https://github.com/sqlparser-rs/sqlparser-rs) and PyO3.
It follows [maturin's recommended mixed Python/Rust project layout](https://www.maturin.rs/project_layout.html).

```
rust/
├── Cargo.toml        # Crate config (pyo3, sqlparser, regex)
└── src/
    ├── lib.rs        # PyO3 module entry — exposes 5 functions as sqlty._core
    ├── analyzer.rs   # SQL type inference via sqlparser-rs
    ├── rewriter.rs   # Python source rewrite/strip via regex
    └── checker.rs    # Lint checks (stale-rewrite, stale-type, SELECT *)
```

Run Rust unit tests independently:

```bash
cargo test --manifest-path rust/Cargo.toml
```

### Test modules

| Module | Purpose |
|--------|---------|
| `tests/conftest.py` | pytest plugin: `.md` discovery, `--update-mdtest` flag |
| `tests/mdtest_runner.py` | Markdown parser + snapshot assertion engine |
| `tests/e2e/conftest.py` | E2E fixtures, CLI runner helpers |

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

- **Documentation**: See [docs/](docs/)
- **Issues**: Report bugs or request features on [GitHub Issues](https://github.com/jirikuncar/sqlty/issues)

## License

MIT License - see [LICENSE](LICENSE) file for details.
