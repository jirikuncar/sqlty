# Magic Codec Example

This example demonstrates the SQLTy magic codec that automatically transforms strings prefixed with `--sql` into `SQL[T](...)` calls with type inference.

## Usage

The magic codec allows you to write SQL queries as simple strings without explicitly calling a function:

```python
# coding: sqlty-magic
from sqlty import magic

q = "--sql SELECT 1::INTEGER"
# Automatically becomes: q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
```

## How It Works

1. **Codec Registration**: The `magic` module registers a custom Python codec called `sqlty-magic`
2. **Source Transformation**: When Python encounters a file with `# coding: sqlty-magic`, it uses the codec to decode the source
3. **String Detection**: The codec detects string literals that start with `--sql`
4. **Type Inference**: It analyzes the SQL query using sqlglot to infer the return type
5. **Transformation**: It wraps the string in `SQL[T](...)` where `T` is the inferred type

## Requirements

- Add `# coding: sqlty-magic` as the first or second line of your file
- Import the magic module: `from sqlty import magic`
- Use `--sql` prefix at the start of your SQL query strings

## Type Inference

The codec infers types based on explicit SQL type casts:

- `--sql SELECT 1::INTEGER` → `SQL[tuple[int]](...)`
- `--sql SELECT id::INTEGER, name::TEXT` → `SQL[tuple[int, str]](...)`
- `--sql SELECT email::TEXT, created_at::TIMESTAMP` → `SQL[tuple[str, datetime.datetime]](...)`

If type inference fails, the codec defaults to `SQL[Any](...)`.

## Running the Example

```bash
# The example file demonstrates the transformation
python examples/magic_codec/demo.py
```

## Notes

- The codec only transforms strings at the source level, before Python parses the code
- The `--sql` prefix remains in the string value
- This is similar to how f-strings work in older Python versions using the `future-fstrings` package
- The transformation happens during the decoding phase, so it works transparently with the Python import system
