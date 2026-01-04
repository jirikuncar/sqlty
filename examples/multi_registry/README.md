# Multi-Registry Example

This example demonstrates organizing SQL queries into separate domain-specific registries.

## File Structure

```
multi_registry/
├── user_queries.py      # User domain queries
├── post_queries.py      # Post domain queries
├── pyproject.toml       # Configuration
└── README.md            # This file
```

## What This Example Shows

- Organizing queries by domain (users, posts)
- Using `@sqlty` decorator for type inference
- Configuration via `pyproject.toml`
- How SQLTy correctly generates separate stub files for each registry

## How It Works

When you run `sqlty` on the `multi_registry` directory:

```bash
cd examples/multi_registry
uv run sqlty .
```

SQLTy generates:
1. `user_queries.pyi` - with only user-related query overloads
2. `post_queries.pyi` - with only post-related query overloads

The separate stub files make it easy to:
- Keep queries organized by domain
- Type-check each module independently
- Avoid conflicts with same function names in different modules

## Architecture

Both `user_queries.py` and `post_queries.py` define a `sql()` function, but SQLTy uses the `pyproject.toml` configuration to:
1. Identify each function uniquely by its import path (`user_queries.sql` vs `post_queries.sql`)
2. Track which queries belong to which module
3. Generate separate, focused stub files for each registry
