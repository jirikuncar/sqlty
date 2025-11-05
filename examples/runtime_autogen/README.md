# Runtime Auto-Generation Example

This example demonstrates SQLTy's runtime auto-generation feature, which automatically creates and updates type stub files when `SQLTY_AUTO_GENERATE=1` is set.

## Features Demonstrated

1. **Single Registry** - Basic query registration with auto-generated stubs
2. **Multiple Registries** - Multiple SQLRegistry classes in a single file
3. **Incremental Stub Generation** - Stubs are updated without losing existing content

**Note:** Only literal string queries are supported for type stub generation.

## Running the Example

### Using uv (recommended)

```bash
cd examples/runtime_autogen
PYTHONPATH=../../src SQLTY_AUTO_GENERATE=1 uv run demo.py
```

### Using make

```bash
cd examples/runtime_autogen
make run
```

This will:
- Execute the demo script with auto-generation enabled
- Create/update `demo.pyi` with type stubs for all queries
- Display the generated stub file

### Viewing the Generated Stubs

```bash
make show-stub
```

### Cleaning up

```bash
make clean
```

## What Happens

When you run the script with `SQLTY_AUTO_GENERATE=1`:

1. **Class Definition**: When a `SQLRegistry` subclass is defined, the initial stub is generated
2. **Runtime Tracking**: All `sql()` calls are tracked during execution
3. **Exit Hook**: At script exit, the stub file is updated with all queries using incremental mode
4. **Type Inference**: SQL type casts (e.g., `::INTEGER`) are analyzed to generate precise Python types

## Expected Output

After running the script, you'll see:
- `demo.pyi` containing type stubs with `@overload` declarations for each query
- Type annotations like `SQL[tuple[int, str]]` based on SQL type casts
- Separate class definitions for UserQueries, PostQueries, and CommentQueries

## Use Cases

This feature is useful for:
- **Development workflows** with auto-reload (e.g., dev servers)
- **Testing** to ensure .pyi stubs stay up-to-date with the codebase
