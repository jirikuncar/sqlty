# Advanced Usage: Multi-Registry, Multiple Schemas, and Configuration

## Function Configuration

SQLTy can analyze functions without requiring the `@sqlty` decorator by configuring them in `pyproject.toml`. This is useful for:

- **Existing codebases** where you don't want to modify source code
- **Third-party libraries** with database helper functions
- **Legacy applications** where adding decorators is impractical
- **Custom function signatures** with non-standard parameter names

### Basic Configuration

Add function configurations to your `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgresql"

[[tool.sqlty.functions]]
import_path = "db.execute_query"
query_param = "query"
schema = "schema.sql"
```

**Required fields:**
- `import_path`: Full import path to the function (e.g., `"module.submodule.function_name"`)
- `query_param`: Name of the parameter containing the SQL query string

**Optional fields:**
- `return_type_param`: Name of the parameter that specifies the expected return type
- `schema`: Path to SQL schema file for type inference
- `return_type_template`: Template for the return type with `{sqlty}` placeholder (e.g., `"list[{sqlty}]"`)
- `imports`: List of import statements needed for the stub

### Example: Function with Return Type Parameter

```toml
[[tool.sqlty.functions]]
import_path = "db.pg_fetchvalue"
query_param = "query"
return_type_param = "return_tp"
schema = "schema.sql"
```

**Usage:**
```python
from db import pg_fetchvalue

# Type determined by return_tp parameter
user_id = pg_fetchvalue(
    conn,
    query="SELECT id FROM users WHERE email = :email",
    return_tp=int,
    email="user@example.com"
)
# Type: int | None
```

**Generated stub:**
```python
@overload
def pg_fetchvalue(
    conn,
    query: Literal["SELECT id FROM users WHERE email = :email"],
    return_tp=int,
    **params
) -> int: ...
```

### Example: Function with Return Type Template

For functions that wrap results in containers (like lists), use `return_type_template`:

```toml
[[tool.sqlty.functions]]
import_path = "db.fetch_all"
query_param = "sql"
return_type_template = "list[{sqlty}]"
schema = "schema.sql"
```

**Usage:**
```python
from db import fetch_all

# Type inferred from SQL and wrapped in list
users = fetch_all(conn, sql="SELECT id::INTEGER, name::TEXT FROM users")
# Type: list[tuple[int, str]]
```

### Example: Custom Imports

If your function requires specific imports for type annotations:

```toml
[[tool.sqlty.functions]]
import_path = "db.query_json"
query_param = "query"
imports = [
    "from typing import Any",
    "import json"
]
```

### Limitations

- **Class methods** (e.g., `"module.Class.method"`) are not currently supported
- Use module-level functions instead
- The `self_type_template` field is reserved for future method support

### Complete Example

See [`examples/function_config/`](https://github.com/jirikuncar/sqlty/tree/main/examples/function_config) for a working example with:
- Database helper functions without decorators
- Configuration in `pyproject.toml`
- Generated type stubs
- Full mypy integration

---

## Multi-Registry and Multiple Schema Support

You can organize queries into multiple registries, each with its own schema file for type inference. Use the unified `--schema` option to provide schemas:

```bash
uv run sqlty examples/multi-schema/users_queries.py --schema users_queries:users.sql
uv run sqlty examples/multi-schema/orders_queries.py --schema orders_queries:orders.sql
```

Or for a package or directory:

```bash
uv run sqlty examples/multi-schema/ --schema users_queries:users.sql --schema orders_queries:orders.sql --mode=full
```

You can also provide a single global schema for all registries:

```bash
uv run sqlty examples/schema-based/ --schema schema.sql --mode=full
```

Schema precedence for a given registry:

1. `--schema registry:PATH` (registry-specific)
2. `--schema PATH` (global)
3. `__schema_path__` on the decorated function

This ensures explicit CLI mappings always win over in-code defaults.

## Configuration via pyproject.toml

You can configure SQLTy in your `pyproject.toml` under `[tool.sqlty]`:

```toml
[tool.sqlty]
dialect = "sqlite"
schemas = { users_queries = "users.sql", orders_queries = "orders.sql" }
```

CLI arguments always take precedence over configuration file values.

## Full Example: Multi-Registry Package

See `examples/multi_schema_pkg/` for a complete package example:

- `users_queries.py` with `users.sql`
- `orders_queries.py` with `orders.sql`
- `app.py` using both registries
- `pyproject.toml` with schema mappings

Stub generation:

```bash
uv run sqlty examples/multi_schema_pkg/ --mode=full
```

Usage:

```python
from users_queries import sql as user_sql
from orders_queries import sql as order_sql

user_result = user_sql("SELECT id, name FROM users")  # SQL[tuple[int, str]]
order_result = order_sql("SELECT id, total FROM orders")  # SQL[tuple[int, float]]
```

## Formatting Generated Stubs

You can have SQLTy pipe the generated stub content through an external formatter. The generator writes the stub text to the formatter's stdin and expects the formatted result on stdout. The command must include `{filename}` placeholder; this expands to the target stub path and is useful for tools that use the filename for configuration or heuristics.

### Ruff (Recommended)

```toml
[tool.sqlty]
# ... your other settings (dialect, schemas, etc.)
format-command = "ruff format --stdin-filename {filename} -"
```

### Black

```toml
[tool.sqlty]
format-command = "black -q --stdin-filename {filename} -"
```

## Type Inference Examples

### SQL-Based Type Inference

```python
# Your code with explicit SQL type casts
user_query = sql("""
    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id
""")

# Generator automatically infers: SQL[tuple[int, str]]
```

The generator will analyze the SQL query, detect the `::INTEGER` and `::TEXT` type casts, and generate:

```python
def sql(
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id"],
) -> SQL[tuple[int, str]]: ...
```

This means mypy will enforce that you handle the result as `tuple[int, str]`!

### Automatic Stub Generation (Development Mode)

For development convenience, you can enable automatic stub generation by setting the `SQLTY_AUTO_GENERATE` environment variable:

```bash
export SQLTY_AUTO_GENERATE=1
python my_app.py  # Stubs will be auto-generated on first import
```

This will automatically generate or update stub files whenever a decorator is used. The feature uses incremental mode by default, so it won't overwrite your existing stubs but will add new queries as they're discovered.

**Note:** Auto-generation is intended for development only. In production, generate stubs once and commit them to your repository for faster startup times.
