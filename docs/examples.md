# Examples Guide

This guide provides complete working examples, organized from simplest to most complex. Each example builds on previous concepts while introducing new patterns.

## Getting Started: Which Example Should I Use?

- **New to SQLTy?** Start with [Basic Query with Type Casts](#1-basic-query-with-type-casts)
- **Have schema files?** Jump to [Schema-Based Type Inference](#2-schema-based-type-inference)
- **Can't modify source code?** See [Function Configuration](#3-function-configuration-no-decorators)
- **Multiple databases?** See [Multiple Schemas](#4-multiple-schemas)
- **Large application?** Check [Multi-Registry Pattern](#5-multi-registry-pattern)
- **Production setup?** Review [Full Package Integration](#6-full-package-integration)

---

## 1. Basic Query with Type Casts

**Location:** [`examples/query/`](https://github.com/jirikuncar/sqlty/tree/main/examples/query)

The simplest approach - use explicit SQL type casts (`::TYPE`) for type inference. This is the best starting point.

**What you'll learn:**
- Creating a basic `@sqlty` decorated function
- Using type casts for explicit typing
- Generating and using stub files

### File Structure

```
examples/query/
├── queries.py      # Registry definition
├── query.py        # Main application
├── queries.pyi     # Generated stub file
└── schema.sql      # Optional schema
```

### Code

**queries.py:**
```python
from typing import Any
from sqlty import sqlty

@sqlty
def sql(query: str) -> Any:
    """Execute SQL queries."""
    return query
```

**query.py:**
```python
from .queries import sql

query = sql(
    """
    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id
    """
)
# Type: SQL[tuple[int, str]]

# Execute with full type safety
from sqlty.drivers.psycopg import execute
result = execute(conn, query, {"id": 1})
# Type: Iterator[tuple[int, str]]

for user_id, name in result:
    print(f"User {user_id}: {name}")
```

### Generate Stubs

```bash
sqlty examples/query/ --mode=full
```

**Best for:** Quick start, simple queries, learning SQLTy basics

---

## 2. Schema-Based Type Inference

**Location:** [`examples/schema_based/`](https://github.com/jirikuncar/sqlty/tree/main/examples/schema_based)

Use a schema file to avoid explicit type casts - types are inferred automatically from `schema.sql`.

**What you'll learn:**
- Using `__schema_path__` for automatic type inference
- Cleaner SQL without explicit casts
- Schema file management

### File Structure

```
examples/schema_based/
├── query.py        # Registry with __schema_path__
├── query.pyi       # Generated stub
└── schema.sql      # Database schema
```

### Code

**query.py:**
```python
from sqlty import sqlty

@sqlty(schema="schema.sql")  # Relative to this file
def sql(query: str):
    """Execute SQL queries with schema-based inference."""
    return query

# Query using table defined in schema.sql
user_query = sql("SELECT id, name FROM users")
# Type: SQL[tuple[int, str]]  (inferred from schema!)

# No ::TYPE casts needed!
email_query = sql("SELECT email FROM users WHERE id = :id")
# Type: SQL[tuple[str]]  (inferred from schema!)
```

**schema.sql:**
```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL
);
```

### Generate Stubs

```bash
sqlty examples/schema_based/ --mode=full
```

**Best for:** Projects with existing schema files, cleaner SQL queries

---

## 3. Function Configuration (No Decorators)

**Location:** [`examples/function_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/function_config)

Configure functions for SQLTy analysis via `pyproject.toml` instead of using decorators. Perfect for existing codebases, third-party libraries, or when you can't modify source code.

**What you'll learn:**
- Configuring functions via `pyproject.toml`
- Working with existing code without modifications
- Supporting functions with custom parameter names
- Using `return_type_param` for explicit type control

### File Structure

```
examples/function_config/
├── app.py              # Application code using configured functions
├── db.py               # Database utility functions (no decorators)
├── db.pyi              # Generated stub file
├── pyproject.toml      # SQLTy configuration
├── schema.sql          # Database schema for type inference
└── README.md           # Documentation
```

### Configuration

**pyproject.toml:**
```toml
[tool.sqlty]
dialect = "postgresql"

# Function with explicit return type parameter
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "schema.sql"

# Function without return type parameter (infers from SQL)
[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"
```

**Configuration Fields:**
- `import_path` (required): Full import path to the function (e.g., `"module.submodule.function_name"`)
- `query_param` (required): Name of the parameter containing the SQL query string
- `return_type_param` (optional): Name of the parameter specifying the expected return type
- `schema` (optional): Path to SQL schema file for type inference
- `return_type_template` (optional): Template for return type with `{sqlty}` placeholder
- `imports` (optional): List of import statements needed for the stub

**Note:** Class methods (e.g., `"module.Class.method"`) are not currently supported. Use module-level functions instead.

### Code

**db.py (no decorators needed):**
```python
def pg_fetchvalue(conn, query: str, return_tp=None, **params):
    """Fetch a single value from the database."""
    # Implementation details...
    return execute_query(conn, query, params)

def execute_query(conn, query: str, **params):
    """Execute a query and return results."""
    # Implementation details...
    return []
```

**app.py:**
```python
from db import pg_fetchvalue, execute_query

# With return_type_param - type comes from parameter
user_id = pg_fetchvalue(
    conn,
    query="SELECT id FROM users WHERE email = :email",
    return_tp=int,  # This determines the return type
    email="user@example.com"
)
# Type: int | None

# Without return_type_param - types inferred from SQL
results = execute_query(
    conn,
    query="SELECT id::INTEGER, name::TEXT FROM users"
)
# Type: list[tuple[int, str]]
```

### Generate Stubs

```bash
sqlty examples/function_config/
```

This creates `db.pyi` with type-safe overloads:

```python
from typing import overload, Literal
from typing import Any

@overload
def pg_fetchvalue(
    conn,
    query: Literal["SELECT id FROM users WHERE email = :email"],
    return_tp=int,
    **params
) -> int: ...

@overload
def execute_query(
    conn,
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
    **params
) -> list[tuple[int, str]]: ...
```

### Benefits

1. **No code changes**: Works with existing functions and libraries
2. **Type safety**: Full mypy type checking for SQL queries
3. **IDE support**: Autocomplete and type hints
4. **Clean separation**: Configuration separate from implementation
5. **Flexible**: Works with any function signature

### Comparison with Decorator Pattern

| Aspect | Function Config | Decorator Pattern |
|--------|----------------|-------------------|
| **Source modification** | None required | Requires adding `@sqlty` |
| **Configuration** | `pyproject.toml` | In-code with decorator |
| **Use case** | Existing code, libraries | New code, full control |
| **Flexibility** | Custom parameter names | Standard signature |

Both approaches can be used together in the same project.

**Best for:** Legacy codebases, third-party libraries, when you can't modify source code

---

## 4. Multiple Schemas

**Location:** [`examples/multi_schema/`](https://github.com/jirikuncar/sqlty/tree/main/examples/multi_schema)

Organize different database schemas (users, orders) with separate registries and schema files.

**What you'll learn:**
- Managing multiple schemas
- Organizing queries by database/domain
- Using CLI `--schema` flag for registry-specific schemas

### File Structure

```
examples/multi_schema/
├── users_queries.py     # User database queries
├── users_queries.pyi    # Generated stubs
├── users.sql            # User schema
├── orders_queries.py    # Order database queries
├── orders_queries.pyi   # Generated stubs
├── orders.sql           # Order schema
└── main.py              # Application logic
```

### Code

**users_queries.py:**
```python
from sqlty import sqlty

@sqlty(schema="users.sql")
def sql(query: str):
    """Execute user database queries."""
    return query
```

**orders_queries.py:**
```python
from sqlty import sqlty

@sqlty(schema="orders.sql")
def sql(query: str):
    """Execute order database queries."""
    return query
```

**main.py:**
```python
from users_queries import sql as user_sql
from orders_queries import sql as order_sql

# Separate concerns - different schemas
user = user_sql("SELECT id, name FROM users WHERE id = :id")
orders = order_sql("SELECT id, total FROM orders WHERE user_id = :user_id")
```

### Generate Stubs

```bash
# Option 1: Auto-detect from schema parameter
sqlty examples/multi_schema/ --mode=full

# Option 2: Explicit schema mapping
sqlty examples/multi_schema/ --mode=full \
    --schema users_queries:users.sql \
    --schema orders_queries:orders.sql
```

**Best for:** Microservices, multiple databases, logical separation

---

## 5. Multi-Registry Pattern

**Location:** [`examples/multi_registry/`](https://github.com/jirikuncar/sqlty/tree/main/examples/multi_registry)

Advanced pattern for large applications - separate registries by domain (users, posts) for better organization and team collaboration.

**What you'll learn:**
- Domain-driven design with SQLTy
- Organizing queries for large codebases
- Team collaboration patterns

### File Structure

```
examples/multi_registry/
├── user_queries.py      # User domain queries
├── user_queries.pyi     # Generated stubs
├── post_queries.py      # Post domain queries
├── post_queries.pyi     # Generated stubs
└── app.py               # Main application
```

### Code

**user_queries.py:**
```python
from sqlty import sqlty

@sqlty
def sql(query: str):
    """Execute user-related database queries."""
    return query

# User domain queries
get_user = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
list_active_users = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE active = true")
```

**post_queries.py:**
```python
from sqlty import sqlty

@sqlty
def sql(query: str):
    """Execute post-related database queries."""
    return query

# Post domain queries
get_post = sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE id = :id")
list_user_posts = sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")
```

**app.py:**
```python
from user_queries import sql as user_sql
from post_queries import sql as post_sql

# Clear separation of concerns
user = user_sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
posts = post_sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")

# Each team can work on their domain independently
```

### Generate Stubs

```bash
sqlty examples/multi_registry/ --mode=full
```

**Benefits:**
- Clear domain boundaries
- Team can work independently
- Easier to test and maintain
- Scales with application complexity

**Best for:** Large applications, team collaboration, domain-driven design

---

## 6. Full Package Integration

**Location:** [`examples/multi_schema_pkg/`](https://github.com/jirikuncar/sqlty/tree/main/examples/multi_schema_pkg)

Complete package structure showing how to integrate SQLTy into a real Python package with proper configuration, pre-commit hooks, and project setup.

**What you'll learn:**
- Production-ready package structure
- Configuration via `pyproject.toml`
- Pre-commit hook setup
- Complete development workflow

### File Structure

```
examples/multi_schema_pkg/
├── pyproject.toml           # Package configuration with [tool.sqlty]
├── .pre-commit-config.yaml  # Pre-commit hook configuration
├── orders.sql               # Order schema
├── users.sql                # User schema
├── orders_queries.py        # Order queries
├── orders_queries.pyi       # Generated stubs
├── users_queries.py         # User queries
├── users_queries.pyi        # Generated stubs
└── app.py                   # Main application
```

### Configuration

**pyproject.toml:**
```toml
[project]
name = "my-app"
requires-python = ">=3.14"
dependencies = ["sqlty"]

[tool.sqlty]
dialect = "postgres"
mode = "full"

# Schema mappings
[[tool.sqlty.schema]]
registry = "users_queries"
path = "users.sql"

[[tool.sqlty.schema]]
registry = "orders_queries"
path = "orders.sql"
```

**.pre-commit-config.yaml:**
```yaml
repos:
  - repo: https://github.com/jirikuncar/sqlty
    rev: v0.1.0
    hooks:
      - id: sqlty
```

### Generate Stubs

```bash
# Using configuration from pyproject.toml
sqlty .

# Or manually with all options
sqlty . --mode=full --dialect=postgres \
    --schema users_queries:users.sql \
    --schema orders_queries:orders.sql
```

### Development Workflow

1. **Initial setup:**
   ```bash
   uv sync
   pre-commit install
   ```

2. **Write queries:**
   ```python
   from users_queries import sql as user_sql
   query = user_sql("SELECT id, name FROM users")
   ```

3. **Generate stubs automatically on commit:**
   ```bash
   git add users_queries.py
   git commit -m "Add user query"
   # Pre-commit hook runs sqlty automatically
   ```

4. **Type check:**
   ```bash
   mypy .
   ```

**Best for:** Production applications, understanding full integration, team projects

---

## Running the Examples

All examples can be run directly from the repository:

```bash
# Clone the repository
git clone https://github.com/jirikuncar/sqlty.git
cd sqlty

# Install dependencies
uv sync

# Run an example
uv run examples/query/query.py

# Generate stubs for an example
uv run sqlty examples/query/ --mode=full

# Type check an example
uv run mypy examples/query/
```

### Automatic Stub Generation for Examples

The repository is configured with a pre-commit hook that automatically generates stubs for example files when they change. This ensures that the examples always have up-to-date type information.

**How it works:**
- When you modify any Python file in the `examples/` directory
- The pre-commit hook runs `sqlty examples --mode=full --format-command "uv run ruff format --stdin-filename {filename}"`
- All query stubs are regenerated from scratch
- Generated stubs are formatted with ruff
- The updated `.pyi` files are automatically included in your commit

**To enable this in your own projects:**

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: sqlty-examples
        name: sqlty stub generation for examples
        entry: uv run sqlty examples --mode=full --format-command "uv run ruff format --stdin-filename {filename}"
        language: system
        pass_filenames: false
        require_serial: true
        types: [python]
        files: ^examples/
```

This configuration can be adapted to any directory structure - just change `examples` to your target directory and adjust the `files` pattern accordingly.

## Next Steps

- Read the [User Guide](index.md) for comprehensive documentation
- Check the [API Reference](api.md) for detailed API documentation
- See [Advanced Usage](advanced-usage.md) for more complex patterns
- Review the [Installation Guide](installation.md) for setup options
