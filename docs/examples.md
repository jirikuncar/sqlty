# Examples Guide

This guide provides complete working examples, organized from simplest to most complex. Each example builds on previous concepts while introducing new patterns.

## Getting Started: Which Example Should I Use?

- **New to SQLTy?** Start with [Basic Query with Type Casts](#1-basic-query-with-type-casts)
- **Have schema files?** Jump to [Schema-Based Type Inference](#2-schema-based-type-inference)
- **Multiple databases?** See [Multiple Schemas](#3-multiple-schemas)
- **Large application?** Check [Multi-Registry Pattern](#4-multi-registry-pattern)
- **Production setup?** Review [Full Package Integration](#5-full-package-integration)

---

## 1. Basic Query with Type Casts

**Location:** [`examples/query/`](https://github.com/jirikuncar/sqlty/tree/main/examples/query)

The simplest approach - use explicit SQL type casts (`::TYPE`) for type inference. This is the best starting point.

**What you'll learn:**
- Creating a basic SQLRegistry
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
from sqlty import SQLRegistry

class Query(SQLRegistry):
    """Application SQL queries."""
    pass

sql = Query.sql
```

**query.py:**
```python
from .queries import Query as Q

query = Q.sql(
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
from sqlty import SQLRegistry

class SchemaQueries(SQLRegistry):
    __schema_path__ = "schema.sql"  # Relative to this file

sql = SchemaQueries.sql

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

## 3. Multiple Schemas

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
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "users.sql"

sql = UserQueries.sql
```

**orders_queries.py:**
```python
from sqlty import SQLRegistry

class OrderQueries(SQLRegistry):
    __schema_path__ = "orders.sql"

sql = OrderQueries.sql
```

**main.py:**
```python
from users_queries import UserQueries
from orders_queries import OrderQueries

# Separate concerns - different schemas
user = UserQueries.sql("SELECT id, name FROM users WHERE id = :id")
orders = OrderQueries.sql("SELECT id, total FROM orders WHERE user_id = :user_id")
```

### Generate Stubs

```bash
# Option 1: Auto-detect from __schema_path__
sqlty examples/multi_schema/ --mode=full

# Option 2: Explicit schema mapping
sqlty examples/multi_schema/ --mode=full \
    --schema UserQueries:users.sql \
    --schema OrderQueries:orders.sql
```

**Best for:** Microservices, multiple databases, logical separation

---

## 4. Multi-Registry Pattern

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
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    """All user-related database queries."""
    pass

sql = UserQueries.sql

# User domain queries
get_user = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
list_active_users = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE active = true")
```

**post_queries.py:**
```python
from sqlty import SQLRegistry

class PostQueries(SQLRegistry):
    """All post-related database queries."""
    pass

sql = PostQueries.sql

# Post domain queries
get_post = sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE id = :id")
list_user_posts = sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")
```

**app.py:**
```python
from user_queries import UserQueries
from post_queries import PostQueries

# Clear separation of concerns
user = UserQueries.sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
posts = PostQueries.sql("SELECT id::INTEGER, title::TEXT FROM posts WHERE user_id = :user_id")

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

## 5. Full Package Integration

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
registry = "UserQueries"
path = "users.sql"

[[tool.sqlty.schema]]
registry = "OrderQueries"
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
    --schema UserQueries:users.sql \
    --schema OrderQueries:orders.sql
```

### Development Workflow

1. **Initial setup:**
   ```bash
   uv sync
   pre-commit install
   ```

2. **Write queries:**
   ```python
   from users_queries import UserQueries
   query = UserQueries.sql("SELECT id, name FROM users")
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

## Next Steps

- Read the [User Guide](index.md) for comprehensive documentation
- Check the [API Reference](api.md) for detailed API documentation
- See [Advanced Usage](advanced-usage.md) for more complex patterns
- Review the [Installation Guide](installation.md) for setup options
