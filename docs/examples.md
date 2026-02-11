# Examples Guide

Working examples for SQLTy, organized from simplest to most advanced.

## Getting Started: Which Example Should I Use?

- **New to SQLTy?** Start with [Quick Start](#1-quick-start-rewritestrip)
- **Have schema files?** See [Schema-Based Inference](#2-schema-based-inference)
- **Want import-time transformation?** See [Magic Codec](#3-magic-codec)

---

## 1. Quick Start (Rewrite/Strip)

The simplest approach — write `--sql` queries, run `sqlty rewrite .` to add types.

### Workflow

```python
# queries.py — write SQL with --sql prefix and type casts
user_query = "--sql SELECT id::INTEGER, name::TEXT FROM users"
count_query = "--sql SELECT COUNT(*)::INTEGER FROM users"
```

```bash
# Add type annotations
sqlty rewrite queries.py
```

```python
# queries.py — after rewrite
from sqlty import SQL

user_query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
count_query = SQL[tuple[int]]("--sql\nSELECT COUNT(*)::INTEGER FROM users")
```

```bash
# Verify type annotations are correct
sqlty check queries.py

# Strip annotations back to plain strings (for editing SQL)
sqlty strip queries.py
```

**Best for:** Getting started, simple projects, learning SQLTy

---

## 2. Schema-Based Inference

Use a database schema file for type inference — no explicit `::TYPE` casts needed.

### Configuration

```toml
# pyproject.toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

### Code

```python
# queries.py — no explicit casts needed
user_query = "--sql SELECT id, name, email FROM users"
```

```bash
sqlty rewrite --schema schema.sql queries.py
```

```python
# queries.py — types inferred from schema
from sqlty import SQL

user_query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

**Best for:** Projects with existing schema files, cleaner SQL without casts

---

## 3. Magic Codec

**Location:** [`examples/magic_codec/`](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec)

Import-time transformation — `--sql` strings are automatically rewritten when the file is imported.

### Code

```python
# coding: sqlty

# --sql strings transformed at import time
user_query = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# At runtime: SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

count_query = "--sql SELECT COUNT(*)::INTEGER FROM users"
# At runtime: SQL[tuple[int]]("--sql\nSELECT COUNT(*)::INTEGER FROM users")
```

### Setup

The codec is automatically registered when sqlty is installed (via `sqlty_magic.pth`).

**Best for:** Development, prototyping, rapid iteration

See [magic codec documentation](magic-codec.md) for details.

---

## Running Examples

All examples can be run directly from the repository:

```bash
# Clone the repository
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty

# Install dependencies
uv sync

# Rewrite example files
sqlty rewrite examples/magic_codec/

# Check example files
sqlty check examples/magic_codec/

# Strip example files
sqlty strip examples/magic_codec/
```

## Pre-commit Integration

The repository uses pre-commit hooks to keep examples in sync:

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-check
        name: sqlty check examples
        entry: uv run sqlty check examples/
        language: system
        pass_filenames: false
        types: [python]
        files: ^examples/
      - id: sqlty-rewrite
        name: sqlty rewrite examples
        entry: uv run sqlty rewrite examples/
        language: system
        pass_filenames: false
        types: [python]
        files: ^examples/
```

## Next Steps

- Read the [User Guide](index.md) for comprehensive documentation
- Check the [API Reference](api.md) for detailed CLI and type documentation
- See [Usage Patterns](patterns.md) for workflow recommendations
