# Advanced Usage

## Custom SQL Dialects

SQLTy supports multiple SQL dialects via sqlglot:

```bash
# PostgreSQL (default)
sqlty rewrite . --dialect postgres

# MySQL
sqlty rewrite . --dialect mysql

# SQLite
sqlty rewrite . --dialect sqlite
```

Configure the dialect in `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "sqlite"
```

## Multiple Schemas

For projects with multiple databases, use named schema registries:

```toml
# pyproject.toml
[tool.sqlty.schemas]
users = "db/users.sql"
orders = "db/orders.sql"
```

Or provide a single schema via CLI:

```bash
sqlty rewrite --schema schema.sql .
sqlty check --schema schema.sql .
```

## Schema-Based Type Inference

When a schema is available, SQLTy can infer types without explicit `::TYPE` casts:

```sql
-- schema.sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
```

```python
# No casts needed — types inferred from schema
q = "--sql\nSELECT id, name, email FROM users"

# After rewrite:
q = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")
```

### Schema Precedence

When resolving schemas:

1. `--schema <PATH>` CLI flag (highest priority)
2. `[tool.sqlty] schema = "..."` in `pyproject.toml`
3. `[tool.sqlty.schemas]` named registries

CLI flags always override config file values.

## Return Formats

The `return-format` option controls how types are represented:

```toml
[tool.sqlty]
return-format = "typeddict"  # or "tuple", "namedtuple", "pydantic"
```

| Format | Result for `SELECT id::INTEGER, name::TEXT` |
|--------|---------------------------------------------|
| `tuple` (default) | `tuple[int, str]` |
| `typeddict` | `QueryRow_{hash}` (TypedDict) |
| `namedtuple` | `QueryRow_{hash}` (namedtuple) |
| `pydantic` | `QueryRow_{hash}` (BaseModel) |

Per-file override via coding declaration:

```python
# coding: sqlty?return-format=typeddict
```

## Formatting Rewritten Files

Pipe rewritten output through an external formatter:

```toml
[tool.sqlty]
format-command = "ruff format --stdin-filename {filename} -"
```

## Lint Rule Configuration

Configure rule severity in `pyproject.toml`:

```toml
[tool.sqlty.rules]
select-star = "ignore"           # Suppress SELECT * warnings
column-name-mismatch = "error"   # Treat name mismatches as errors
stale-rewrite = "warn"           # Warn on unwrapped --sql strings
```

Valid severities: `"error"`, `"warn"`, `"ignore"`.

## Pre-commit Integration

Use `sqlty check` and `sqlty rewrite` as pre-commit hooks:

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-check
        name: sqlty check
        entry: uv run sqlty check .
        language: system
        pass_filenames: false
        types: [python]
      - id: sqlty-rewrite
        name: sqlty rewrite
        entry: uv run sqlty rewrite .
        language: system
        pass_filenames: false
        types: [python]
```

## Deferred Features

The following features are planned but not yet implemented:

- **Stub generation** (`sqlty generate`) — Generate `.pyi` overload stubs from configured functions
- **Structured return types** — Protocol-based typing for TypedDict/namedtuple/Pydantic
- **Function configuration** — `[[tool.sqlty.functions]]` for analyzing existing functions without code changes

See [PLAN-FUTURE.md](../PLAN-FUTURE.md) for details.
