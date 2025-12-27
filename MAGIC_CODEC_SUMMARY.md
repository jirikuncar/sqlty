# Magic Codec Implementation Summary

## Overview

This document summarizes the implementation of the magic codec feature for SQLTy, which allows strings prefixed with `--sql` to be automatically transformed into `SQL[T](...)` calls with type inference.

## Implementation Details

### Files Created

1. **src/sqlty/magic.py** (203 lines)
   - Main codec implementation
   - Registers `sqlty-magic` and `sqlty_magic` codecs
   - Provides `decode()` function for source transformation
   - Includes `IncrementalDecoder` and `StreamReader` classes for codec API
   - Uses SQLAnalyzer for type inference

2. **tests/test_magic.py** (78 lines)
   - Unit tests for `_infer_sql_type()` function
   - Unit tests for `_transform_sql_string()` function
   - Tests various string formats (single, double, triple quotes)
   - Tests edge cases (whitespace, non-SQL strings)

3. **tests/test_magic_integration.py** (98 lines)
   - Integration tests for the codec
   - Tests codec registration
   - Tests decode function behavior
   - Tests preservation of non-SQL strings

4. **tests/test_magic_issue.py** (102 lines)
   - Tests specifically for the GitHub issue example
   - Demonstrates the exact use case from the issue
   - Tests with and without explicit type casts

5. **examples/magic_codec/demo.py** (60 lines)
   - Example demonstrating the magic codec usage
   - Shows various SQL transformations
   - Includes documentation comments

6. **examples/magic_codec/README.md** (51 lines)
   - Documentation for the magic codec feature
   - Usage instructions
   - Examples and notes

### Files Modified

1. **src/sqlty/__init__.py**
   - Added `magic` to `__all__` list

2. **README.md**
   - Added magic codec section to features
   - Added usage example
   - Added note about experimental status

## How It Works

### Codec Registration

When `from sqlty import magic` is imported, the module automatically registers the `sqlty-magic` codec with Python's codec system using `codecs.register()`.

### Source Transformation

1. **Encoding Declaration**: Files using the codec must include `# coding: sqlty-magic` as the first or second line
2. **Detection**: The codec uses Python's `tokenize` module to parse the source code
3. **Transformation**: String tokens starting with `--sql` are detected and transformed
4. **Type Inference**: The SQL query is analyzed using SQLAnalyzer to infer the return type
5. **Wrapping**: The string is wrapped in `SQL[T](...)` where `T` is the inferred type

### Example Transformation

**Input:**
```python
# coding: sqlty-magic
from sqlty import magic

q = "--sql SELECT 1::INTEGER"
```

**Output (after codec transformation):**
```python
# coding: sqlty-magic
from sqlty import magic

q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
```

## Type Inference

The codec uses the existing SQLAnalyzer to infer types:

- `--sql SELECT 1::INTEGER` → `SQL[tuple[int]]`
- `--sql SELECT id::INTEGER, name::TEXT` → `SQL[tuple[int, str]]`
- `--sql SELECT created_at::TIMESTAMP` → `SQL[tuple[datetime.datetime]]`
- `--sql SELECT unknown_query` → `SQL[Any]` (fallback when inference fails)

## Testing

A comprehensive test suite was created with 19 tests covering:

- Type inference for various SQL queries
- String transformation with different quote styles
- Edge cases (whitespace, non-SQL strings)
- Integration with Python's codec system
- The exact use case from the GitHub issue

All tests pass, and the implementation is fully type-checked with mypy.

## Limitations and Notes

1. **Experimental Feature**: This is an experimental feature inspired by future-fstrings
2. **Requires Explicit Casts**: For accurate type inference, SQL queries should use explicit type casts (e.g., `::INTEGER`)
3. **Source-Level Transformation**: The transformation happens at the source code level, before Python parses the code
4. **Encoding Declaration Required**: Files must include `# coding: sqlty-magic` to use this feature
5. **Import Side Effect**: Importing `magic` registers the codec globally

## Future Enhancements

Possible improvements for future versions:

1. Support for schema-aware type inference (using schema files)
2. More sophisticated fallback types when casts are missing
3. Integration with stub generation workflow
4. Support for parameterized queries
5. Better error messages for malformed SQL

## Conclusion

The magic codec implementation successfully addresses the GitHub issue by providing a convenient way to write SQL queries as simple strings while maintaining type safety through automatic transformation and type inference.
