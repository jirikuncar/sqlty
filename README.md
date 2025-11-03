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
sqlty . --mode=full
```

**Type check:**

```bash
mypy .
# Success: no issues found
```

### Pre-commit Hook

Automatically generate stubs on commit by adding to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: v0.1.0  # Use the latest version
    hooks:
      - id: sqlty
        # Optional: customize with args
        # args: [".", "--mode=full", "--schema=schema.sql"]
```

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
