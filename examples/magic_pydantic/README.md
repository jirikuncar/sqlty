# Magic Codec with Pydantic Format

This example demonstrates the SQLTy magic codec configured to use Pydantic return format.

## Configuration

There are two ways to configure the return format:

### 1. Via pyproject.toml (directory-level)

The `pyproject.toml` in this directory configures the return format:

```toml
[tool.sqlty]
return_format = "pydantic"
```

### 2. Via coding declaration (file-level)

You can override the format in individual files using query parameters in the coding declaration:

```python
# coding: sqlty?return-format=pydantic
```

This overrides any `pyproject.toml` configuration for that specific file.

## How It Works

When using the magic codec with Pydantic format:

1. The `# coding: sqlty` declaration enables the codec
2. Optionally add `?return-format=pydantic` to specify format for this file
3. Strings prefixed with `--sql` are automatically transformed
4. The codec generates Pydantic BaseModel classes with proper field names and types
5. Query results are typed as the generated Pydantic model

## Example

```python
# coding: sqlty?return-format=pydantic
from pydantic import BaseModel
from sqlty import SQL

class QueryRow_abc123(BaseModel):
    id: int
    name: str

query: SQL[QueryRow_abc123] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
```

## Running

```bash
python examples/magic_pydantic/demo.py
python examples/magic_pydantic/demo_with_coding.py
```

## Type Checking

The generated Pydantic models can be verified with mypy:

```bash
cd examples/magic_pydantic
mypy demo.py
mypy demo_with_coding.py
```
