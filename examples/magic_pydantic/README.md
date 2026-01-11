# Magic Codec with Pydantic Format

This example demonstrates the SQLTy magic codec configured to use Pydantic return format.

## Configuration

The `pyproject.toml` in this directory configures the return format:

```toml
[tool.sqlty]
return_format = "pydantic"
```

## How It Works

When using the magic codec with Pydantic format:

1. The `# coding: sqlty` declaration enables the codec
2. Strings prefixed with `--sql` are automatically transformed
3. The codec generates Pydantic BaseModel classes with proper field names and types
4. Query results are typed as the generated Pydantic model

## Example

```python
# coding: sqlty
from sqlty import SQL

query: SQL = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Becomes: SQL[QueryRow_abc123]("...")
# where QueryRow_abc123 is:
#   class QueryRow_abc123(BaseModel):
#       id: int
#       name: str
```

## Running

```bash
python examples/magic_pydantic/demo.py
```

## Type Checking

The generated Pydantic models can be verified with mypy:

```bash
cd examples/magic_pydantic
mypy demo.py
```
