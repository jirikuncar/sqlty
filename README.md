# SQLTy

> ⚠️ **Experimental Package**: This is an experimental project under active development. APIs may change without notice. Feedback and contributions are welcome!

Type-safe SQL queries for Python with automatic stub generation and runtime validation.

[![Python 3.14+](https://img.shields.io/badge/python-3.14+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Powered by [sqlglot](https://sqlglot.com)** - SQL parsing and analysis engine that makes type inference possible across multiple SQL dialects.

## Overview

SQLTy provides type-safe SQL query execution with automatic type inference and stub generation. Write SQL queries with explicit type casts, and let SQLTy generate precise type stubs for full mypy integration.

```python
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql

# Type-safe query with automatic inference
query = sql("SELECT id::INTEGER, name::TEXT FROM users")
# Type: SQL[tuple[int, str]]

# Execute with full type safety
from sqlty.drivers.psycopg import execute
result = execute(conn, query, {})
# Type: Iterator[tuple[int, str]]

# Full mypy support
for user_id, name in result:
    print(f"User {user_id}: {name}")
```

## Features

- 🔒 **Type Safety**: Full mypy integration with precise type inference
- 🚀 **Automatic Stub Generation**: Analyze code and generate `.pyi` files automatically
- 🎯 **SQL Type Casts**: Use PostgreSQL-style `::TYPE` casts for explicit typing
- 📦 **Multi-Registry Support**: Organize queries into multiple registries
- 🔍 **Source Tracking**: Generated stubs include source location comments
- 🌐 **Multiple SQL Dialects**: Support for PostgreSQL, MySQL, SQLite, and more via sqlglot

## Quick Start

**Install:**

```bash
pip install sqlty
# or
uv add sqlty
```

**Write queries:**

See [advanced usage documentation](https://github.com/jirikuncar/sqlty/blob/main/docs/advanced-usage.md) for:

- Multi-registry and multiple schema support
- Configuration via pyproject.toml
- Full package example with stub generation and usage

```python
# queries.py
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
```

```python
# app.py
from queries import sql
from sqlty.drivers.psycopg import execute

query = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
# Type: SQL[tuple[int, str]]

result = execute(conn, query, {"id": 123})
# Type: Iterator[tuple[int, str]]
```

**Generate stubs:**

```bash
# Single file (incremental updates)
sqlty queries.py

# Directory (incremental + prune stale queries + create missing stubs)
sqlty .

# Only create missing stubs (skip existing)
sqlty . --mode=create

# Custom mode combinations
sqlty . --mode=incremental,prune
```

**Check stubs are up-to-date:**

```bash
# Check without modifying files
sqlty . --check

# Check with detailed diff output
sqlty . --check --verbose

# Check with colored diff (requires rich library)
sqlty . --check -vv
```

**Type check:**

```bash
mypy .
# Success: no issues found
```

## Stub Generation Modes

SQLTy supports flexible stub generation with comma-separated mode actions:

- **`incremental`**: Add new query overloads to existing stubs
- **`prune`**: Remove overloads for queries that no longer exist
- **`create`**: Only create missing stub files (skip existing ones)

**Default behavior:**
- Single file: `incremental,create`
- Directory: `incremental,prune,create`

**Examples:**
```bash
# Update existing stubs and remove stale queries
sqlty . --mode=incremental,prune

# Create new stubs but skip existing ones
sqlty . --mode=create

# Full regeneration (replace all overloads)
sqlty queries.py --mode=fresh
```

**Safety:** Prune mode prompts for confirmation when used on single files to prevent accidentally removing stubs for queries defined elsewhere.

## Check Mode

The `--check` flag verifies that stub files are up-to-date without modifying them. This is useful for CI/CD pipelines and pre-commit hooks to ensure type stubs match the current code.

**Note:** `--check` and `--format-command` are mutually exclusive. Check mode compares the intermediate representations (IR) of registries - specifically the query literals and return types of each overload - rather than text output. This provides semantic comparison that ignores formatting, whitespace, and comment differences, ensuring only meaningful changes trigger failures.

```bash
# Basic check (exit code 0 if up-to-date, 1 if differences found)
sqlty . --check

# Show unified diff of changes
sqlty . --check --verbose

# Show colored diff with syntax highlighting (requires rich library)
sqlty . --check -vv
```

### Pre-commit Hook

Automatically generate or check stubs on commit by adding to your `.pre-commit-config.yaml`:

**Generate stubs automatically:**
```yaml
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: v0.1.0  # Use the latest version
    hooks:
      - id: sqlty
        # Optional: customize with args
        # Default mode for directories: incremental,prune,create
        # args: [".", "--schema=schema.sql"]
```

**Check stubs are up-to-date (recommended for CI):**
```yaml
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: v0.1.0  # Use the latest version
    hooks:
      - id: sqlty-check
        # Shows verbose diff output when stubs are outdated
        # args: [".", "--check", "--verbose"]
```

For more targeted stub generation (e.g., only for specific directories), you can use a local hook:

```yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-src
        name: sqlty stub generation
        entry: sqlty . --format-command "ruff format --stdin-filename {filename}"
        language: system
        pass_filenames: false
        require_serial: true
        types: [python]
        files: ^src/  # Only run on files in src/
```

This approach allows you to:
- Automatic mode selection based on directory vs file
- Use `--format-command` to format generated stubs with your preferred formatter
- Target specific directories with the `files` pattern
- Customize behavior per directory or project area

## Documentation

📚 **[Complete Documentation](https://github.com/jirikuncar/sqlty/blob/main/docs/README.md)**

### Quick Links

- **[Installation Guide](https://github.com/jirikuncar/sqlty/blob/main/docs/installation.md)** - Setup instructions for pip, uv, poetry, and more
- **[User Guide](https://github.com/jirikuncar/sqlty/blob/main/docs/index.md)** - Complete guide with basic and advanced usage
- **[API Reference](https://github.com/jirikuncar/sqlty/blob/main/docs/api.md)** - Detailed API documentation for all classes and methods
- **[Examples Guide](https://github.com/jirikuncar/sqlty/blob/main/docs/examples.md)** - Step-by-step tutorials from basic to advanced

### Key Topics

- [Basic Usage](https://github.com/jirikuncar/sqlty/blob/main/docs/index.md#basic-usage) - Creating registries, writing queries, generating stubs
- [Multi-Registry Pattern](https://github.com/jirikuncar/sqlty/blob/main/docs/index.md#multi-registry-pattern) - Organizing queries for large applications
- [Type Inference](https://github.com/jirikuncar/sqlty/blob/main/docs/index.md#type-inference) - How SQLTy infers Python types from SQL
- [Internals](https://github.com/jirikuncar/sqlty/blob/main/docs/index.md#internals) - Architecture and implementation details

## Contributing

See **[DEVELOPMENT.md](https://github.com/jirikuncar/sqlty/blob/main/docs/DEVELOPMENT.md)** for development setup, workflows, and contribution guidelines.

## License

MIT License - see [LICENSE](https://github.com/jirikuncar/sqlty/blob/main/LICENSE) file for details.
