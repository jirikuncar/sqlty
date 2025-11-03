# Development Guide

This guide covers development setup, workflows, and contribution guidelines for SQLTy.

## Prerequisites

- Python 3.14+
- [uv](https://docs.astral.sh/uv/) - Fast Python package installer and resolver

## Setup

### Install Dependencies

```bash
uv sync
```

This will install all dependencies and development tools defined in `pyproject.toml`.

### Install Pre-commit Hooks

```bash
uv run pre-commit install
```

This ensures code is formatted and checked before each commit.

#### Pre-commit Hooks Configured

The repository uses several pre-commit hooks:

- **Standard checks**: AST validation, YAML/TOML/JSON syntax, merge conflicts, trailing whitespace, etc.
- **Ruff**: Code formatting and linting with automatic fixes
- **uv-lock**: Keeps the lockfile up to date
- **mypy**: Type checking for the src directory
- **sqlty-examples**: Automatic stub generation for example files
  - Runs incrementally when any Python file in `examples/` changes
  - Uses `--mode=incremental` to add new overloads without regenerating existing ones
  - Ensures example stubs stay synchronized with code changes

To manually run all pre-commit hooks:

```bash
uv run pre-commit run --all-files
```

## Development Workflow

### Type Checking

Run mypy type checking on the source code and examples:

```bash
make typecheck
```

Or directly:

```bash
uv run mypy src/sqlty examples
```

### Running Tests

```bash
uv run pytest
```

With coverage:

```bash
uv run pytest --cov=src/sqlty --cov-report=html
```

### Code Formatting

Format code with Ruff:

```bash
uv run ruff format .
```

Lint code:

```bash
uv run ruff check .
```

Fix linting issues automatically:

```bash
uv run ruff check --fix .
```

## Generating Type Stubs

The `sqlty` tool analyzes your Python code to automatically generate `.pyi` stub files with `@overload` declarations. It infers return types from:

1. **SQL Analysis**: Uses [sqlglot](https://sqlglot.com) to parse SQL queries and extract column types from explicit type casts (e.g., `column::INTEGER`)
2. **Schema Files**: Optionally load database schema (via `--schema` option) for enhanced type inference

### Generate Stubs

```bash
make generate-stubs
```

Or directly:

```bash
uv run sqlty <source_file_or_directory> [options]
```

### CLI Options

- `-o, --output`: Output .pyi file path (auto-detected based on imports by default)
- `--dialect`: SQL dialect for analysis (default: postgres). Supports: postgres, mysql, sqlite, bigquery, snowflake, and [more](https://sqlglot.com/sqlglot.html#supported-dialects)
- `--mode=incremental`: Add new overloads to existing stub instead of replacing
- `--mode=full`: Remove overloads for queries that no longer exist (auto-enabled for directory mode)
- `--schema PATH` or `--schema registry:PATH`: Provide schema files to improve type inference. Repeat `--schema` for multiple registry mappings. Precedence: registry-specific > global > `__schema_path__` on the registry class.

### Examples

```bash
# Single file mode - generates stub alongside source
uv run sqlty examples/query/query.py

# Directory mode - scans all files and generates central stub
uv run sqlty examples/query/ --mode=full

# Incremental mode - add new queries without regenerating everything
uv run sqlty examples/query/new_queries.py --mode=incremental
```

## Running Examples

Run any example directly:

```bash
uv run examples/query/query.py
```

Or use the Makefile:

```bash
make run-example
```


## Contributing

### Development Process

1. **Fork and clone** the repository
2. **Create a branch** for your feature/fix
3. **Make changes** with tests
4. **Run tests** and type checking
5. **Commit** with a clear message
6. **Push** and create a pull request

### Code Style

- Follow PEP 8 and PEP 484 (type hints)
- Use Ruff for formatting and linting
- Add type hints to all functions
- Write docstrings for public APIs

### Testing

- Add tests for new features
- Ensure all tests pass
- Maintain or improve code coverage

### Pull Request Guidelines

- Write clear PR descriptions
- Reference related issues
- Ensure CI passes
- Request review from maintainers

## Roadmap

### Priority Tasks

Based on user value and implementation effort:

1. **Schema-based examples** (High Value, Low Effort)
   - Many users want to avoid explicit casts
   - Schema files are common in real projects
   - Demonstrates --schema flag

2. **Auto-generate examples** (High Value, Low Effort)
   - Improves developer experience
   - Already implemented, just needs example
   - Shows SQLTY_AUTO_GENERATE env var

3. **Incremental mode examples** (Medium Value, Low Effort)
   - Important for large codebases
   - Shows --mode=incremental workflow
   - Relatively simple to demonstrate

4. **Migration examples** (High Value, Medium Effort)
   - Real-world use case
   - Helps adoption
   - Requires more setup/explanation

5. **Dialect-specific examples** (Low Value, Medium Effort)
   - Niche use case
   - Already documented in CLI
   - Can be added later

### TODO List

- [ ] Automatically add new @overload entries on `sql(...)` calls when SQLTY_AUTO_GENERATE=1 is set. Ensure minimal overhead and file locking to prevent race conditions.
- [ ] Support more psycopg query types (Composed, Nested, etc.)
- [ ] Support UUID type mapping
- [ ] Support datetime and interval type mappings
- [ ] Ensure locking is in place for concurrent stub generation in multi-threaded environments or multiple processes (file locks)
- [x] Allow composable flags --mode=incremental,prune or --mode=full
  - [x] Implemented --mode flag with incremental, full, and prune options
  - [x] Removed legacy --mode=incremental and --mode=full flags (breaking change)
  - [x] Updated API: generate_stub_file now uses mode parameter instead of boolean flags
  - [x] Configure it to run on changed files in examples
  - [ ] Prune implementation (remove overloads for queries that no longer exist)
- [ ] Make sure everything works with both `ty` and `mypy`.
- [ ] Make sub-commands or parameters to check pyi files for missing or outdated stubs.
- [ ] Add bootstrap sub-command to generate registry skeleton, detect schema if available in repo, and suggests configuration for pyproject.toml and pre-commit if present.
- [ ] Add caching of parsed schema files to improve performance on repeated runs.
- [ ] Cache generated stubs to avoid regenerating unchanged files.

### Ideas

- [ ] Support more psycopg query types (e.g. Composed, Nested, ...)
- [ ] Migration tool for existing codebases

## Getting Help

- **Documentation**: See [docs/](https://github.com/jirikuncar/sqlty/tree/main/docs)
- **Issues**: Report bugs or request features on [GitHub Issues](https://github.com/jirikuncar/sqlty/issues)
- **Discussions**: Ask questions in [GitHub Discussions](https://github.com/jirikuncar/sqlty/discussions)

## License

MIT License - see [LICENSE](LICENSE) file for details.
