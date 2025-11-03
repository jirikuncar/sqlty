# End-to-End Tests for SQLTy

This directory contains end-to-end tests for sqlty that validate the complete workflow without importing internal modules (those starting with `_`).

## Testing Approach

These tests follow the acceptance criteria:

1. **Define source files** with repositories and queries
2. **Run pyi generator** via subprocess (simulating real usage)
3. **Snapshot result** using inline-snapshot for verification
4. **Run mypy** on queries to ensure expected type annotations are present
5. **Perform changes** (optional) and validate incremental behavior

## Test Files

### `test_basic_workflow.py`
Tests the basic sqlty workflow:
- Simple query generation with type casts
- Explicit type annotations
- Incremental stub updates
- Multiline query formatting

### `test_multi_registry.py`
Tests multi-registry patterns:
- Multiple registries in different files
- Same query in different registries
- Nested package structures

### `test_mypy_validation.py`
Tests mypy integration:
- Type validation on generated stubs
- Type mismatch detection
- Tuple unpacking type inference
- Multiple queries with different types
- Parameterized queries
- Complex types

### `test_dialects_and_schema.py`
Tests SQL dialect support and schema-based inference:
- PostgreSQL dialect
- MySQL dialect  
- Schema-based type inference
- Fallback to generic types
- Mixed typed/untyped queries
- UUID type handling

### `test_cli_modes.py`
Tests CLI commands and modes:
- Help and version commands
- Full mode (analyze all files)
- Incremental mode (changed files only)
- Specific file generation
- Output path options
- Dialect and schema options
- Error handling

### `test_driver_integration.py`
Tests integration with database drivers:
- psycopg driver pattern with type annotations
- Iterator return types
- Parameterized query type safety
- Multiple registries in driver code

### `test_configuration.py`
Tests configuration via pyproject.toml:
- Basic configuration settings
- Schema path configuration
- CLI arguments override config
- Multiple registry configuration
- Exclude patterns

### `test_edge_cases.py`
Tests edge cases and error scenarios:
- Empty projects
- No queries found
- Syntax errors in source
- Non-literal query strings
- Duplicate queries
- Very long queries
- Unicode in queries
- SQL comments
- Nested function calls
- Class method queries
- Queries in conditionals

## Key Principles

### No Internal Imports
Tests do NOT import any modules starting with `_`:
- ❌ `from sqlty._generator import TypeStubGenerator`
- ❌ `from sqlty._sql_analyzer import SQLAnalyzer`
- ✅ `subprocess.run([sys.executable, "-m", "sqlty", ...])`

### Inline Snapshots
Results are verified using `inline-snapshot`:
```python
from inline_snapshot import snapshot

assert stub_content == snapshot("""expected content here""")
```

### Mypy Validation
Tests run mypy to ensure generated stubs work correctly:
```python
mypy_result = subprocess.run(
    [sys.executable, "-m", "mypy", str(app_file), "--strict"],
    capture_output=True,
    text=True,
)
assert mypy_result.returncode == 0
```

## Running the Tests

Run all end-to-end tests:
```bash
pytest tests/e2e/
```

Run a specific test file:
```bash
pytest tests/e2e/test_basic_workflow.py
```

Run a specific test:
```bash
pytest tests/e2e/test_basic_workflow.py::test_basic_query_generation
```

Update snapshots when behavior changes intentionally:
```bash
pytest tests/e2e/ --inline-snapshot=create
```

or to update existing snapshots:
```bash
pytest tests/e2e/ --inline-snapshot=update
```

## Benefits of This Approach

1. **True End-to-End Testing**: Tests exercise the actual CLI and workflow users experience
2. **No Internal Dependencies**: Tests won't break from internal refactoring
3. **Type Safety Validation**: Mypy integration ensures generated stubs actually work
4. **Snapshot Testing**: Easy to review changes and maintain expected outputs
5. **Realistic Scenarios**: Tests mimic real-world usage patterns

## Writing New Tests

When adding new tests:

1. Create temporary test files in `tmp_path`
2. Run `sqlty` via subprocess
3. Verify generated stub files exist
4. Use `snapshot()` to capture expected content
5. Run mypy to validate type safety
6. Test edge cases and error conditions

Example template:
```python
def test_new_feature(tmp_path: Path) -> None:
    """Test description."""
    # 1. Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''...''')
    
    app_file = tmp_path / "app.py"
    app_file.write_text('''...''')
    
    # 2. Run generator
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    
    # 3. Verify and snapshot
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()
    stub_content = stub_file.read_text()
    assert stub_content == snapshot("""...""")
    
    # 4. Run mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )
    assert mypy_result.returncode == 0
```
