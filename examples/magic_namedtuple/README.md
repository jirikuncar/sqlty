# Magic Codec with namedtuple Format

This example demonstrates the SQLTy magic codec configured to use namedtuple return format.

## Configuration

The `pyproject.toml` in this directory configures the return format:

```toml
[tool.sqlty]
return_format = "namedtuple"
```

## How It Works

When using the magic codec with namedtuple format:

1. The `# coding: sqlty` declaration enables the codec
2. Strings prefixed with `--sql` are automatically transformed
3. The codec generates namedtuple declarations with proper field names
4. Query results are typed as the generated namedtuple

## Example

```python
# coding: sqlty
from sqlty import SQL

query: SQL = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Becomes: SQL[QueryRow_abc123]("...")
# where QueryRow_abc123 is:
#   QueryRow_abc123 = namedtuple('QueryRow_abc123', ['id', 'name'])
```

## Running

```bash
python examples/magic_namedtuple/demo.py
```

## Type Checking

The generated namedtuples can be verified with mypy:

```bash
cd examples/magic_namedtuple
mypy demo.py
```
