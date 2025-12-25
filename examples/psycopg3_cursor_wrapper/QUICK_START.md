# Quick Start Guide

This is a complete working example of psycopg3 cursor wrapper with query-level type inference.

## What This Example Demonstrates

Unlike standard psycopg3 where types are bound to cursors/connections, this example shows how to:

1. **Wrap psycopg3** to provide query-level type inference
2. **Use literal SQL strings** to get precise type information
3. **Execute multiple query types** with a single cursor wrapper
4. **Manually create stub files** with overloads for type checking

## Quick Start

### Option 1: Using the run script

```bash
./run.sh
```

This will:
- Start PostgreSQL in Docker
- Install dependencies
- Run the application

### Option 2: Using Make

```bash
make run
```

Or step by step:

```bash
make install  # Install dependencies
make start    # Start PostgreSQL
make run      # Run the application
make stop     # Stop PostgreSQL when done
```

### Option 3: Manual steps

```bash
# Start PostgreSQL
docker compose up -d

# Install dependencies
uv pip install 'psycopg[binary]'

# Run the application
uv run python app.py

# Stop PostgreSQL
docker compose down
```

## Type Checking

To verify the type inference works:

```bash
# Using mypy
uv run mypy app.py test_types.py

# Using pyright
uv run pyright app.py test_types.py
```

## Key Files

- **[app.py](app.py)** - Main application with TypedCursor wrapper
- **[app.pyi](app.pyi)** - Type stubs with query-level overloads
- **[schema.sql](schema.sql)** - PostgreSQL schema and sample data
- **[docker-compose.yml](docker-compose.yml)** - PostgreSQL container setup
- **[test_types.py](test_types.py)** - Type checking examples

## How It Works

The `TypedCursor` class wraps psycopg3 queries:

```python
cursor = TypedCursor(conn)

# Each literal query gets precise type inference via stubs
result = cursor.execute("SELECT id::INTEGER, name::TEXT FROM users")
# Type: list[tuple[int, str]]
```

The type magic happens in `app.pyi` with overloads:

```python
@overload
def execute(
    self,
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
    params: Params | None = None,
) -> list[tuple[int, str]]: ...
```

## Next Steps

1. Explore the [full README](README.md) for detailed documentation
2. Check [test_types.py](test_types.py) for type checking examples
3. Try modifying queries and regenerating stubs with SQLTy
4. Read about automatic stub generation in the main README

## Comparison with Standard Psycopg3

**Standard approach** (cursor-level typing):
```python
# Need separate connections for different result types
conn1 = psycopg.connect(row_factory=class_row(User))
conn2 = psycopg.connect(row_factory=class_row(Post))
```

**Our approach** (query-level typing):
```python
# Single cursor for all queries
cursor = TypedCursor(conn)
users = cursor.execute("SELECT id, name FROM users")  # list[tuple[int, str]]
posts = cursor.execute("SELECT id, title FROM posts") # list[tuple[int, str]]
```

## Troubleshooting

### PostgreSQL not starting

```bash
docker compose logs postgres
```

### Type errors

Make sure both `app.py` and `app.pyi` exist in the same directory.

### Connection refused

Wait a few seconds after `docker compose up -d` for PostgreSQL to be ready:

```bash
docker compose exec postgres pg_isready -U testuser -d testdb
```
