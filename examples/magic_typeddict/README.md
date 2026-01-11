# Magic Codec with TypedDict Format

This example demonstrates the SQLTy magic codec configured to use TypedDict return format.

## Configuration

There are two ways to configure the return format:

### 1. Via pyproject.toml (directory-level)

The `pyproject.toml` in this directory configures the return format:

```toml
[tool.sqlty]
return_format = "typeddict"
```

### 2. Via coding declaration (file-level)

You can override the format in individual files using query parameters in the coding declaration:

```python
# coding: sqlty?return-format=typeddict
```

This overrides any `pyproject.toml` configuration for that specific file.

## How It Works

When using the magic codec with TypedDict format:

1. The `# coding: sqlty` declaration enables the codec
2. Optionally add `?return-format=typeddict` to specify format for this file
3. Strings prefixed with `--sql` are automatically transformed
4. The codec generates TypedDict classes with proper field names and types
5. Query results are typed as the generated TypedDict class

## Example

```python
# coding: sqlty?return-format=typeddict
from typing import TypedDict
from sqlty import SQL

class QueryRow_abc123(TypedDict):
    id: int
    name: str

query: SQL[QueryRow_abc123] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
```

## Running

```bash
python examples/magic_typeddict/demo.py
python examples/magic_typeddict/demo_with_coding.py
```

## Type Checking

The generated TypedDict classes can be verified with mypy:

```bash
cd examples/magic_typeddict
mypy demo.py
mypy demo_with_coding.py
```
