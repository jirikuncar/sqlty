# Magic Codec with namedtuple Format

This example demonstrates the SQLTy magic codec configured to use namedtuple return format.

## Configuration

There are two ways to configure the return format:

### 1. Via pyproject.toml (directory-level)

The `pyproject.toml` in this directory configures the return format:

```toml
[tool.sqlty]
return_format = "namedtuple"
```

### 2. Via coding declaration (file-level)

You can override the format in individual files using query parameters in the coding declaration:

```python
# coding: sqlty?return-format=namedtuple
```

This overrides any `pyproject.toml` configuration for that specific file.

## How It Works

When using the magic codec with namedtuple format:

1. The `# coding: sqlty` declaration enables the codec
2. Optionally add `?return-format=namedtuple` to specify format for this file
3. Strings prefixed with `--sql` are automatically transformed
4. The codec generates namedtuple declarations with proper field names
5. Query results are typed as the generated namedtuple

## Example

```python
# coding: sqlty?return-format=namedtuple
from collections import namedtuple
from sqlty import SQL

QueryRow_abc123 = namedtuple('QueryRow_abc123', ['id', 'name'])

query: SQL[QueryRow_abc123] = "--sql SELECT id::INTEGER, name::TEXT FROM users"
```

## Running

```bash
python examples/magic_namedtuple/demo.py
python examples/magic_namedtuple/demo_with_coding.py
```

## Type Checking

The generated namedtuples can be verified with mypy:

```bash
cd examples/magic_namedtuple
mypy demo.py
mypy demo_with_coding.py
```
