# Magic Codec

The magic codec allows automatic transformation of strings prefixed with `--sql` into `SQL[T](...)` calls with type inference.

## Overview

```python
q = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Automatically transforms to:
# q = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
```

The codec automatically adds necessary imports when types require them:

```python
q = "--sql SELECT created_at::TIMESTAMP, user_id::UUID"
# Transforms to:
# import datetime
# import uuid
# q = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql SELECT created_at::TIMESTAMP, user_id::UUID")
```

## Installation for Global Use

Due to Python's codec caching, the magic codec requires setup via `sitecustomize.py`:

### Automatic Setup

```bash
python -m sqlty._magic_setup install
```

This creates a `sitecustomize.py` file in your Python's site-packages that automatically enables the codec on startup.

### Manual Setup

Create `sitecustomize.py` in your site-packages directory:

```python
# sitecustomize.py
import sqlty.magic
```

Find your site-packages:
```bash
python -c "import site; print(site.getsitepackages())"
```

## Usage

Once set up, use `--sql` strings in your code:

```python
# No encoding declaration or imports needed in individual files
query = "--sql SELECT id::INTEGER, name::TEXT FROM users"
# Transforms to: SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
```

### Automatic Import Injection

The codec automatically adds imports for types that require them:

- **datetime types** (`DATE`, `TIMESTAMP`, `TIME`, `INTERVAL`) → adds `import datetime`
- **UUID type** → adds `import uuid`
- **Built-in types** (`int`, `str`, `bool`, `float`, `list`, `dict`) → no imports needed

## Type Inference

The codec infers types based on explicit SQL type casts:

- `--sql SELECT 1::INTEGER` → `SQL[tuple[int]](...)`
- `--sql SELECT id::INTEGER, name::TEXT` → `SQL[tuple[int, str]](...)`
- `--sql SELECT email::TEXT, created_at::TIMESTAMP` → `SQL[tuple[str, datetime.datetime]](...)` (+ `import datetime`)
- `--sql SELECT user_id::UUID` → `SQL[tuple[uuid.UUID]](...)` (+ `import uuid`)

Without casts, defaults to `SQL[Any](...)`.

## Testing with the "sqlty" Codec

For explicit testing, the codec is also registered as "sqlty":

```python
# coding: sqlty
import codecs

codec = codecs.lookup("sqlty")
code = b'q = "--sql SELECT 1::INTEGER"'
result, _ = codec.decode(code)
# result: 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
```

This allows testing the codec transformation without affecting UTF-8 globally.

## Direct Usage (Without Global Setup)

Use the `decode()` function to transform source code directly:

```python
from sqlty.magic import decode

code = b'q = "--sql SELECT 1::INTEGER"'
transformed, _ = decode(code)
# transformed: 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
```

## Implementation Details

### Files

- **src/sqlty/magic.py**: Core codec implementation with decode function and type inference
- **src/sqlty/_magic_setup.py**: Helper script for installing/uninstalling sitecustomize.py
- **tests/test_magic*.py**: Comprehensive test suite (19 tests)
- **examples/magic_codec/**: Example code and documentation

### How It Works

1. **Codec Registration**: The magic module registers a codec search function
2. **Source Transformation**: When Python decodes a file, the codec detects `--sql` strings  
3. **Type Inference**: Uses SQLAnalyzer to infer return types from SQL queries
4. **Transformation**: Wraps strings in `SQL[T](...)` where `T` is the inferred type

### Technical Limitations

- Python's codec registry caches lookups; built-in codecs (like UTF-8) are registered before user code runs
- The codec must be registered very early (via sitecustomize.py) to work reliably
- Cannot override UTF-8 after Python startup due to caching
- Alternative: use `decode()` function directly for source transformation

## Uninstallation

```bash
python -m sqlty._magic_setup uninstall
```

## Status Check

```bash
python -m sqlty._magic_setup status
```

## Examples

See [examples/magic_codec/](../examples/magic_codec/) for complete examples.
