# Magic Codec

Import-time transformation: `--sql` strings are automatically rewritten when the module is imported.

## Usage

Add `# coding: sqlty` at the top of your file:

```python
# coding: sqlty

user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"
# At runtime: SQL[tuple[int, str]]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

No `sqlty rewrite` step needed. The codec transforms strings at import time.

## How it works

1. The `# coding: sqlty` declaration tells Python to use SQLTy's custom codec
2. When Python decodes the source file, the codec detects `--sql` string literals
3. It analyzes the SQL query and infers return types
4. The string is wrapped in `SQL[T](...)` before Python parses the code

## Type inference

The codec infers types from explicit `::TYPE` casts:

- `"--sql\nSELECT 1::INTEGER"` -> `SQL[tuple[int]](...)`
- `"--sql\nSELECT id::INTEGER, name::TEXT"` -> `SQL[tuple[int, str]](...)`
- `"--sql\nSELECT created_at::TIMESTAMP"` -> `SQL[tuple[datetime.datetime]](...)`

If type inference fails (invalid SQL, DML without RETURNING), the string is left unchanged.

## Running the example

```bash
python examples/magic_codec/demo.py
```

## Best for

Development and prototyping. Use `sqlty rewrite` for production code.
