# Magic Codec with TypedDict Format

This example demonstrates the SQLTy magic codec configured to use TypedDict return format.

## Configuration

The `pyproject.toml` in this directory configures the return format:

```toml
[tool.sqlty]
return_format = "typeddict"
```

## How It Works

When using the magic codec with TypedDict format:

1. The `# coding: sqlty` declaration enables the codec
2. Strings prefixed with `--sql` are automatically transformed
3. The codec generates TypedDict classes with proper field names and types
4. Query results are typed as the generated TypedDict class

## Example

```python
# coding: sqlty
from sqlty import SQL

query: SQL = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Becomes: SQL[QueryRow_abc123]("...")
# where QueryRow_abc123 is:
#   class QueryRow_abc123(TypedDict):
#       id: int
#       name: str
```

## Running

```bash
python examples/magic_typeddict/demo.py
```

## Type Checking

The generated TypedDict classes can be verified with mypy:

```bash
cd examples/magic_typeddict
mypy demo.py
```
