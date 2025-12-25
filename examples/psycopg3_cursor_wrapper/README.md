# Psycopg3 Cursor Wrapper with Query-Level Type Inference

This example demonstrates how to wrap a psycopg3 cursor to provide type-safe query execution where the return type is inferred from the SQL query string literal, rather than being bound to the cursor itself.

## The Problem with Standard Psycopg3 Typing

Psycopg3 applies generics at the Connection/Cursor level:

```python
conn = psycopg.connect(row_factory=class_row(MyDataClass))
cur = conn.cursor()  # Cursor is typed to return MyDataClass
res = cur.execute("SELECT...").fetchone()  # Always returns MyDataClass
```

**Limitation**: Once a cursor is typed to return `MyDataClass`, ALL queries executed on it must conform to that structure. This means you need separate cursors for different result types.

## Our Solution: Query-Level Type Inference

This example provides a `TypedCursor` wrapper where:

1. **Single cursor, multiple types**: One cursor can execute varied queries with varied return types
2. **Literal query inference**: Type inference happens at the query string level using Literal types
3. **Tuple-based results**: Returns results as tuples (using `TupleRow`) with precise type information
4. **Manual stub files**: Demonstrates type stubs with overloads for each query pattern

## Features

- **TypedCursor wrapper**: Wraps psycopg3 cursor for query-level typing
- **Query literal overloads**: Each SQL literal string gets its own type overload
- **Tuple return types**: Results as `tuple[...]` with inferred column types
- **Context manager support**: Clean resource management with `with` statements
- **Docker setup**: Complete PostgreSQL setup with sample data

## Architecture

```
TypedCursor (wrapper)
    ├── execute(query: str) -> list[tuple[...]]
    │   ├── Overload for Literal["SELECT 1::INTEGER"] -> list[tuple[int]]
    │   ├── Overload for Literal["SELECT id, name FROM users"] -> list[tuple[int, str]]
    │   └── Fallback for non-literal str -> list[tuple[Any, ...]]
    │
    └── execute_one(query: str) -> tuple[...] | None
        ├── Similar overloads for single-row queries
        └── Returns None if no row found
```

## File Structure

```
psycopg3_cursor_wrapper/
├── docker-compose.yml      # PostgreSQL container setup
├── schema.sql             # Database schema and sample data
├── app.py                 # Main application with TypedCursor
├── app.pyi                # Type stub with query overloads
├── pyproject.toml         # Project configuration
└── README.md              # This file
```

## Setup and Running

### 1. Start PostgreSQL

```bash
docker compose up -d
```

Wait for PostgreSQL to be ready (healthcheck should pass):

```bash
docker compose ps
```

### 2. Install Dependencies

```bash
# From the example directory
uv pip install psycopg[binary]
```

Or if using the parent sqlty project:

```bash
cd /Users/jirikuncar/src/sqlty
uv pip install -e ".[psycopg]"
```

### 3. Run the Application

```bash
python app.py
```

### 4. Type Check

```bash
# Using mypy
mypy app.py

# Using pyright
pyright app.py
```

## How It Works

### The Wrapper Class

The `TypedCursor` class wraps a psycopg3 cursor:

```python
class TypedCursor:
    def __init__(self, conn: Connection[TupleRow]) -> None:
        self._conn = conn
        self._cursor = conn.cursor()

    def execute(self, query: str, params: Params | None = None) -> Any:
        self._cursor.execute(query, params)
        return self._cursor.fetchall()
```

At runtime, this is just a simple wrapper that delegates to the underlying cursor.

### Type Stub Overloads

The magic happens in `app.pyi` with overloads:

```python
@overload
def execute(
    self,
    query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
    params: Params | None = None,
) -> list[tuple[int, str]]: ...

@overload
def execute(self, query: str, params: Params | None = None) -> list[tuple[Any, ...]]: ...
```

When you write:

```python
result = cursor.execute("SELECT id::INTEGER, name::TEXT FROM users")
```

The type checker sees the literal string and matches it to the first overload, inferring `list[tuple[int, str]]`.

For non-literal queries (e.g., runtime-constructed strings), the fallback overload returns `list[tuple[Any, ...]]`.

## Example Queries

### Simple Query

```python
result = cursor.execute("SELECT 1::INTEGER")
# Type: list[tuple[int]]
```

### Schema-Based Query

```python
result = cursor.execute("SELECT id::INTEGER, name::TEXT FROM users")
# Type: list[tuple[int, str]]
```

### Single Row Query

```python
result = cursor.execute_one("SELECT id::INTEGER, name::TEXT FROM users WHERE id = 1")
# Type: tuple[int, str] | None
```

### Complex Join Query

```python
result = cursor.execute(
    """
    SELECT u.name::TEXT, p.title::TEXT, p.published::BOOLEAN
    FROM users u
    JOIN posts p ON u.id = p.user_id
    WHERE p.published = true
    """
)
# Type: list[tuple[str, str, bool]]
```

## Comparison with Standard Psycopg3

### Standard Approach (Cursor-Level Typing)

```python
# Need separate cursors for different result types
conn = psycopg.connect(row_factory=class_row(User))
user_cursor = conn.cursor()  # Returns User objects

conn2 = psycopg.connect(row_factory=class_row(Post))
post_cursor = conn2.cursor()  # Returns Post objects
```

### Our Approach (Query-Level Typing)

```python
# Single cursor for all query types
cursor = TypedCursor(conn)
users = cursor.execute("SELECT id, name FROM users")  # list[tuple[int, str]]
posts = cursor.execute("SELECT id, title FROM posts")  # list[tuple[int, str]]
```

## Generating Stubs with SQLTy

While this example uses manual stub files, SQLTy can generate these automatically:

1. Configure in `pyproject.toml`:

```toml
[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "app.TypedCursor.execute"
query_param = "query"
schema = "schema.sql"

[[tool.sqlty.functions]]
import_path = "app.TypedCursor.execute_one"
query_param = "query"
schema = "schema.sql"
```

2. Run SQLTy:

```bash
sqlty . --mode=full
```

This would automatically generate the overloads in `app.pyi` based on the queries found in your code.

## Advantages

1. **Granular typing**: Each query gets its own precise type
2. **Single cursor**: No need to manage multiple cursors or connections
3. **Type safety**: Full IntelliSense and type checking support
4. **Flexibility**: Mix literal and runtime queries in the same cursor
5. **Clean code**: Natural API that feels like standard psycopg3

## Limitations

1. **Literal strings only**: Type inference only works for literal query strings
2. **Manual stubs**: Without SQLTy automation, you need to maintain stubs manually
3. **Stub file sync**: Need to keep `.pyi` in sync with actual queries in code

## Next Steps

- Explore automatic stub generation with SQLTy
- Add support for more complex query patterns
- Integrate with connection pooling
- Add transaction management to the wrapper
- Support for prepared statements with typing

## Resources

- [Psycopg3 Documentation](https://www.psycopg.org/psycopg3/docs/)
- [SQLTy Documentation](https://github.com/jirikuncar/sqlty)
- [Python typing with Literal](https://docs.python.org/3/library/typing.html#typing.Literal)
- [PEP 612 - Parameter Specification Variables](https://www.python.org/dev/peps/pep-0612/)
