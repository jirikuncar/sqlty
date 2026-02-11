# SQLTy

> **Experimental Package**: This is an experimental project under active development. APIs may change without notice. Feedback and contributions are welcome!

Type-safe SQL queries for Python with automatic type inference, source rewriting, and lint checks.

[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Powered by [sqlglot](https://sqlglot.com)** - SQL parsing and analysis engine that makes type inference possible across multiple SQL dialects.

## Overview

SQLTy infers Python types from SQL queries and provides tools to rewrite, strip, and check your codebase for type safety. Write SQL queries with explicit type casts, and let SQLTy handle the rest.

```python
# Before: plain --sql strings
query = "--sql SELECT id::INTEGER, name::TEXT FROM users"

# After: sqlty rewrite .
from sqlty import SQL
query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Key operations

| Command | What it does | Analogy |
|---------|-------------|---------|
| `sqlty rewrite .` | `"--sql ..."` -> `SQL[T]("--sql\n...")` in source files | `ruff format` |
| `sqlty strip .` | Reverse: `SQL[T]("--sql\n...")` -> `"--sql\n..."` | (unique to sqlty) |
| `sqlty check .` | Lint: type mismatches, SELECT *, stale rewrites | `ruff check` |

## Why SQLTy?

**For teams working with raw SQL in large codebases.** If you're using an ORM, stick with its type system.

- **Minimal runtime overhead** - Type checking happens at analysis time, not runtime
- **Fast imports** - Stub-based typing avoids expensive runtime introspection
- **Scale** - Designed for large codebases with thousands of queries

See [motivation documentation](docs/motivation.md) for details.

## Quick Start

**Install:**

```bash
pip install sqlty
# or
uv add sqlty
```

**Write queries with `--sql` prefix:**

```python
# queries.py
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
```

**Rewrite to add type annotations:**

```bash
sqlty rewrite queries.py
```

This transforms the file to:

```python
# queries.py
from sqlty import SQL
q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

**Strip annotations (reverse):**

```bash
sqlty strip queries.py
```

**Check for issues:**

```bash
sqlty check .
```

## Features

- **Type Inference**: Infers Python types from SQL casts (`::INTEGER` -> `int`, `::TEXT` -> `str`)
- **Rewrite/Strip**: Bidirectional source transformation between `"--sql ..."` and `SQL[T]("--sql\n...")`
- **Lint Rules**: 8 built-in rules with configurable severity (ty-style `error`/`warn`/`ignore`)
- **Star Expansion**: `SELECT *` -> explicit column list using schema
- **Pluggable Codecs**: Custom type formatting via `TypeCodec` protocol
- **Multiple SQL Dialects**: PostgreSQL, MySQL, SQLite, and more via sqlglot
- **Schema-aware**: Load `CREATE TABLE` schemas for enhanced inference
- **Optional Rust core**: Compiled via PyO3/maturin for faster type inference (falls back to pure Python)

### Lint rules

| Rule | Default | Description |
|------|---------|-------------|
| `column-count-mismatch` | error | Annotation has different field count than query |
| `column-type-mismatch` | error | Annotation field type differs from inferred type |
| `column-name-mismatch` | warn | Annotation field name differs from column name |
| `select-star` | warn | `SELECT *` used (fixable: expand to explicit columns) |
| `select-star-no-schema` | error | `SELECT *` used but no schema available |
| `stale-rewrite` | warn | `SQL[T](...)` annotation is outdated |
| `stale-type` | warn | Type annotation doesn't match current inference |
| `inline-sql-comment` | warn | `"--sql SELECT ..."` makes query a SQL comment (fixable) |

Configure in `pyproject.toml`:

```toml
[tool.sqlty.rules]
select-star = "ignore"
column-name-mismatch = "error"
```

## Magic Codec

SQLTy includes an optional codec that transforms `--sql` strings at import time:

```python
# Strings with --sql prefix are automatically transformed
q = "--sql SELECT 1::INTEGER"
# Becomes: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")

# Automatically adds imports for datetime/uuid types
q = "--sql SELECT created_at::TIMESTAMP, user_id::UUID"
# Becomes:
# import datetime
# import uuid
# q = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql\nSELECT created_at::TIMESTAMP, user_id::UUID")
```

See [magic codec documentation](docs/magic-codec.md) for setup and details.

## Benchmarks

SQLTy ships an optional Rust extension (compiled via PyO3/maturin) that accelerates all core operations. When not available, a pure-Python fallback is used automatically.

| Benchmark | Python | Rust | Speedup |
|-----------|-------:|-----:|--------:|
| `infer_type`: 1 column | 120 us | 5 us | **24x** |
| `infer_type`: 4 columns | 340 us | 12 us | **28x** |
| `infer_type`: 10 cols + JOINs | 1,340 us | 54 us | **25x** |
| `infer_type`: schema-aware | 1,770 us | 53 us | **33x** |
| `expand_star`: with schema | 1,700 us | 51 us | **33x** |
| `rewrite_source`: 1 query | 245 us | 10 us | **25x** |
| `rewrite_source`: 3 queries | 600 us | 25 us | **24x** |
| `rewrite_source`: 20 queries | 5,930 us | 230 us | **26x** |
| `strip_source`: 1 query | 4 us | 3 us | 1.5x |
| `strip_source`: 20 queries | 40 us | 45 us | 0.9x |
| `check_source`: 4 queries | 415 us | 24 us | **17x** |

**Geometric mean speedup: ~15x** (median of two runs on Linux x86_64).

`strip_source` is regex-only (no SQL parsing), so both backends perform similarly. All other operations — which involve SQL parsing and type inference — see 17-33x speedups.

Run benchmarks yourself:

```bash
uv run python benchmarks/bench_core.py
```

## Testing

SQLTy uses a markdown-based test framework (mdtest) inspired by [ty's mdtest](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md). Tests are snapshot-based `.md` files in `tests/mdtest/`:

```bash
# Run all tests
make test

# Run only mdtests
make test-mdtest

# Run only unit tests (no e2e, no mdtest)
make test-unit

# Run only e2e tests
make test-e2e

# Update mdtest snapshots with actual values
make update-mdtest
```

Browse `tests/mdtest/` to see sqlty's behavior specified as readable markdown test suites.

## Documentation

- **[Installation Guide](docs/installation.md)** - Setup instructions
- **[User Guide](docs/index.md)** - Complete guide with basic and advanced usage
- **[API Reference](docs/api.md)** - Detailed API documentation
- **[Development Guide](DEVELOPMENT.md)** - Contributing, testing, architecture

## Contributing

See **[DEVELOPMENT.md](DEVELOPMENT.md)** for development setup, workflows, and contribution guidelines.

## License

MIT License - see [LICENSE](LICENSE) file for details.
