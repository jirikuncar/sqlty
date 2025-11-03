# SQLTy Test Structure

This document describes the test structure and the refactored testing approach.

## Test Directories

### `tests/e2e/` ✅ **RECOMMENDED**
End-to-end tests that follow best practices:
- **No internal imports**: Does not import from modules starting with `_`
- **Subprocess execution**: Tests the actual CLI that users interact with
- **Inline snapshots**: Uses `inline-snapshot` for result verification
- **Mypy validation**: Ensures generated stubs work with mypy type checking

**This is the recommended location for new tests.**

See [tests/e2e/README.md](e2e/README.md) for detailed information.

### `tests/integration/` ⚠️ **DEPRECATED**
Integration tests that import from internal modules (`sqlty._generator`).
These tests violate the acceptance criteria and should not be extended.
See [tests/integration/DEPRECATED.md](integration/DEPRECATED.md).

### `tests/generator/` ⚠️ **INTERNAL ONLY**  
Unit tests for the generator implementation.
These import from `sqlty._generator` and test internal details.
Useful for development but not for end-to-end validation.
See [tests/generator/NOTICE.md](generator/NOTICE.md).

### `tests/sql_analyzer/` ⚠️ **INTERNAL ONLY**
Unit tests for the SQL analyzer implementation.
These import from `sqlty._sql_analyzer` and test internal details.
Useful for development but not for end-to-end validation.
See [tests/sql_analyzer/NOTICE.md](sql_analyzer/NOTICE.md).

## Testing Philosophy

### What Should Be Tested in E2E Tests?

1. **User-facing workflows**
   - Running `sqlty` CLI on source code
   - Generating .pyi stub files
   - Validating with mypy

2. **Public API contracts**
   - SQLRegistry behavior
   - SQL class behavior
   - Generated stub format and accuracy

3. **Complete scenarios**
   - Basic queries with type casts
   - Multi-registry patterns
   - Different SQL dialects
   - Schema-based type inference
   - Incremental updates

### What Should Be Tested in Unit Tests?

Unit tests (in `generator/` and `sql_analyzer/`) are still useful for:
- Testing internal helper functions
- Edge cases in parsing logic
- Performance-sensitive code paths
- Internal data structures

However, these should be considered implementation details and not the primary
test suite for validating correctness.

## Running Tests

### Run all end-to-end tests (recommended)
```bash
pytest tests/e2e/ -v
```

### Run all tests
```bash
pytest tests/
```

### Run with coverage
```bash
pytest tests/ --cov=sqlty --cov-report=html
```

### Update inline snapshots
When output format changes intentionally:
```bash
pytest tests/e2e/ --inline-snapshot=update
```

## Migration Guide

If you're working on existing tests that import internal modules:

### Before (❌ Don't do this)
```python
from sqlty._generator import TypeStubGenerator

def test_something(tmp_path):
    generator = TypeStubGenerator()
    result = generator.generate_stub_file(source_file)
    assert result.exists()
```

### After (✅ Do this instead)
```python
import subprocess
import sys
from inline_snapshot import snapshot

def test_something(tmp_path):
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()
    assert stub_file.read_text() == snapshot("""...""")
```

## Benefits of the New Approach

1. **Tests the actual user experience**: Uses the CLI as users would
2. **More maintainable**: Less coupled to internal implementation
3. **Better documentation**: Tests serve as usage examples
4. **Type-safe validation**: Mypy checks ensure generated stubs actually work
5. **Snapshot testing**: Easy to review and update expected outputs

## Contributing

When adding new features:
1. Write end-to-end tests in `tests/e2e/` first
2. Add unit tests in `tests/generator/` or `tests/sql_analyzer/` if needed for internal validation
3. Update snapshots with `--inline-snapshot=update` after verifying changes
4. Ensure mypy validation passes in e2e tests

### Example Template

See [tests/e2e/test_example_template.py](e2e/test_example_template.py) for a complete
template showing:
- Basic test pattern with snapshots
- Tests without mypy validation
- Error case testing
- CLI option testing
- Incremental behavior testing

Copy and modify the template when adding new test cases.

### Tips for Writing Tests

1. **Use descriptive test names** that explain what is being tested
2. **Add docstrings** explaining the test purpose and any special considerations
3. **Use tmp_path** for all file operations to ensure isolation
4. **Assert success before checking content** to catch failures early
5. **Use snapshot()** for first run, then update with `--inline-snapshot=update`
6. **Include mypy validation** for type-safety tests
7. **Test both success and failure scenarios** to ensure robustness
8. **Keep tests focused** on one aspect at a time for clarity
9. **Document assumptions** about feature implementation status
10. **Use clear variable names** like `queries_file`, `app_file`, `stub_file`

For questions or clarification, see the original issue: 
"Refactor tests to use inline-snapshot and mypy check"
