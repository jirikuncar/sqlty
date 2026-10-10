# Magic Codec

The magic codec allows automatic transformation of strings prefixed with `--sql` into `SQL[T](...)` calls with type inference.

## Overview

```python
q = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# Automatically transforms to:
# q = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The codec automatically adds necessary imports when types require them:

```python
q = "--sql\nSELECT created_at::TIMESTAMP, user_id::UUID"
# Transforms to:
# import datetime
# import uuid
# from sqlty import SQL
# q = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql\nSELECT created_at::TIMESTAMP, user_id::UUID")
```

## Installation for Global Use

When you install sqlty via pip or uv, a `sqlty_magic.pth` file is automatically installed in your site-packages directory. This file enables the magic codec globally by importing `sqlty.magic` on Python startup.

### Automatic Installation

```bash
pip install sqlty
# or
uv add sqlty
```

The `sqlty_magic.pth` file is automatically placed in your site-packages, enabling the codec for all Python sessions.

### Disabling the Magic Codec

If you want to disable automatic codec registration, set the `SQLTY_NO_MAGIC` environment variable:

```bash
export SQLTY_NO_MAGIC=1
```

Or in Python code before importing anything:

```python
import os
os.environ['SQLTY_NO_MAGIC'] = '1'
```

### Verify Installation

Check if the `.pth` file is installed:

```bash
python -c "import site; print([p for p in site.getsitepackages()])"
# Look for sqlty_magic.pth in the site-packages directory
```

## Usage

Once set up, use `--sql` strings in your code:

```python
query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# decoded: query = SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Automatic Import Injection

The codec automatically adds imports for types that require them:

- **datetime types** (`DATE`, `TIMESTAMP`, `TIME`, `INTERVAL`) → adds `import datetime`
- **UUID type** → adds `import uuid`
- **Built-in types** (`int`, `str`, `bool`, `float`, `list`, `dict`) → no imports needed

## Type Inference

The codec infers types based on explicit SQL type casts:

- `--sql\nSELECT 1::INTEGER` → `SQL[tuple[int]](...)`
- `--sql\nSELECT id::INTEGER, name::TEXT` → `SQL[tuple[int, str]](...)`
- `--sql\nSELECT email::TEXT, created_at::TIMESTAMP` → `SQL[tuple[str, datetime.datetime]](...)` (+ `import datetime`)
- `--sql\nSELECT user_id::UUID` → `SQL[tuple[uuid.UUID]](...)` (+ `import uuid`)

Without casts or for non-result-set queries (DML without RETURNING, DDL), the string is left unchanged.

## Testing with the "sqlty" Codec

For explicit testing, the codec is also registered as "sqlty":

```python
# coding: sqlty
import codecs

codec = codecs.lookup("sqlty")
code = b'q = "--sql\\nSELECT 1::INTEGER"'
result, _ = codec.decode(code)
# result: 'from sqlty import SQL\n\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
```

This allows testing the codec transformation without affecting UTF-8 globally.

## Direct Usage (Without Codec Registration)

For programmatic transformation without codec registration, import from `sqlty._decode`:

```python
from sqlty._decode import decode  # No codec registration!

code = b'q = "--sql\\nSELECT 1::INTEGER"'
transformed, _ = decode(code)
# transformed: 'from sqlty import SQL\n\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
```

Or use the CLI tool:

```bash
python -m sqlty.magic input.py > output.py
```

## Implementation Details

### Files

- **src/sqlty/_decode.py**: Core decode logic (delegates to `SQLRewriter` for type inference)
- **src/sqlty/_rewriter.py**: Source-level rewrite/strip engine
- **src/sqlty/magic.py**: Codec registration and setup
- **sqlty_magic.pth**: Auto-installed file that enables codec on Python startup
- **tests/mdtest/codec/basic_codec.md**: Snapshot-based mdtest suite
- **tests/test_magic*.py**: Integration tests
- **examples/magic_codec/**: Example code and documentation

### How It Works

1. **Installation**: When you `pip install sqlty`, the `sqlty_magic.pth` file is placed in site-packages
2. **Startup**: Python reads `.pth` files on startup and executes `import sqlty.magic`
3. **Registration**: The magic module registers a codec search function (unless `SQLTY_NO_MAGIC=1`)
4. **Source Transformation**: When Python decodes files with `# coding: sqlty`, it uses the custom codec
5. **Type Inference**: Uses SQLAnalyzer to infer return types from SQL queries
6. **Transformation**: Wraps `--sql` strings in `SQL[T](...)` and adds necessary imports

### Architecture

```
sqlty/_decode.py        - Pure decode logic (no side effects)
       ↑
       |
sqlty/magic.py          - Codec registration + imports decode
       ↑
       |
sqlty_magic.pth         - Auto-imported on Python startup
```

### Technical Notes

- Python's codec registry caches lookups; the codec must be registered very early
- The `.pth` file ensures registration happens before any other code runs
- Use `SQLTY_NO_MAGIC=1` to disable if needed
- Alternative: import from `sqlty._decode` for no-side-effects decode

## Disabling or Uninstalling

### Temporary Disable

```bash
export SQLTY_NO_MAGIC=1
python your_script.py
```

### Permanent Disable

Remove the `.pth` file from your site-packages:

```bash
python -c "import site; print(site.getsitepackages())"
# Then manually delete sqlty_magic.pth from that directory
```

Or uninstall sqlty:

```bash
pip uninstall sqlty
```

## Examples

See [examples/magic_codec/](https://github.com/jirikuncar/sqlty/tree/main/examples/magic_codec) for complete examples.
