# Runtime Auto-Generation Example

This example demonstrates SQLTy's runtime auto-generation feature, which automatically creates and updates type stub files when `SQLTY_AUTO_GENERATE=1` is set.

## Running the Example

### Run

```bash
cd examples/runtime_autogen
make clean
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

## What Happens

When you run the script with `SQLTY_AUTO_GENERATE=1`:

1. **Runtime Tracking**: All `sql()` calls are tracked during execution
2. **Exit Hook**: At script exit, the stub file is updated with all queries using incremental mode
4. **Type Inference**: SQL type casts (e.g., `::INTEGER`) are analyzed to generate precise Python types

## Expected Output

After running the script, you'll see:
- `pyi` file containing type stubs with `@overload` declarations for each query
- Type annotations like `SQL[tuple[int, str]]` based on SQL type casts

## Use Cases

This feature is useful for:
- **Development workflows** with auto-reload (e.g., dev servers)
- **Testing** to ensure .pyi stubs stay up-to-date with the codebase
