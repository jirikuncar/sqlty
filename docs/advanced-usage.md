# Advanced Usage: Multi-Registry, Multiple Schemas, and Configuration

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
3. `__schema_path__` on the registry class

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

Output:

```
Using schema from examples/multi_schema_pkg/orders.sql (mapping for 'orders_queries')
Using schema from examples/multi_schema_pkg/users.sql (mapping for 'users_queries')
Detected SQLRegistry subclass in 'orders_queries', writing to examples/multi_schema_pkg/orders_queries.pyi
Generated stub file (full): examples/multi_schema_pkg/orders_queries.pyi
Found 2 call(s) to sql
Detected SQLRegistry subclass in 'users_queries', writing to examples/multi_schema_pkg/users_queries.pyi
Generated stub file (full): examples/multi_schema_pkg/users_queries.pyi
Found 2 call(s) to sql

To use this stub file, ensure it's in the same directory as your source or in a location where mypy can find it.

Generated Query class stub with CustomQuery class.
Import and use: from your_module import sql
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

This will automatically generate or update stub files whenever a `SQLRegistry` subclass is imported. The feature uses incremental mode by default, so it won't overwrite your existing stubs but will add new queries as they're discovered.

**Note:** Auto-generation is intended for development only. In production, generate stubs once and commit them to your repository for faster startup times.
