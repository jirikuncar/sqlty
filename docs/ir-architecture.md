# Intermediate Representation (IR) Architecture

## Overview

The SQLTy code generation system has been refactored to use an intermediate representation (IR) for stub file content. This provides a clean separation of concerns and makes the code more maintainable and extensible.

## Architecture

### Three-Phase Pipeline

The new architecture follows a clean three-phase pipeline:

```
Source/Pyi → Parse → IR → Manipulate → IR → Serialize → Output
```

1. **Parsing**: Convert input (source code or .pyi files) to IR
2. **Manipulation**: Perform operations on IR (merge, replace, prune)
3. **Serialization**: Convert IR back to AST and formatted text

### IR Components

#### Core Data Structures (`_stub_ir.py`)

- **`OverloadSignature`**: Represents a single `@overload` method signature
  - `query_literal`: The SQL query string
  - `return_type`: The return type annotation (e.g., `SQL[tuple[int, str]]`)
  - `source_file`, `source_line`: Optional provenance information
  - Equality based on query literal for deduplication

- **`RegistryClass`**: Represents a SQLRegistry subclass with its overloads
  - `name`: Class name (e.g., "UserQueries")
  - `overloads`: Set of `OverloadSignature` objects
  - `export_name`: Name for module-level alias (e.g., "sql", "user_sql")
  - Methods: `add_overload()`, `remove_overload()`, `has_overload()`

- **`ImportStatement`**: Represents an import statement
  - `module`: Module name
  - `names`: List of imported names
  - `from_import`: Boolean flag for "from X import Y" vs "import X"

- **`StubFile`**: Represents the complete structure of a .pyi stub file
  - `imports`: Set of `ImportStatement` objects
  - `registries`: Dictionary mapping class name to `RegistryClass`
  - `file_path`: Optional path to the stub file
  - Methods: `merge_registry()`, `compute_required_imports()`

#### Parsers (`_stub_parser.py`)

- **`PyiToIRParser`**: Parses .pyi files to IR
  - Parses AST nodes to extract imports, classes, overloads, and aliases
  - Handles `@overload` decorators and `Literal[...]` type annotations

- **`CallInfoToIRParser`**: Parses `CallInfo` objects from source analysis to IR
  - Converts collected SQL calls into IR structure
  - Groups calls by registry class
  - Computes required imports based on return types

#### Serializers (`_stub_serializer.py`)

- **`IRToASTSerializer`**: Serializes IR to Python AST
  - Builds proper AST nodes for imports, classes, methods
  - Handles PEP 8 import ordering
  - Generates type-correct AST for all constructs

- **`IRToTextSerializer`**: Serializes IR to formatted Python text
  - Uses `IRToASTSerializer` to build AST
  - Adds appropriate blank lines between sections
  - Produces PEP 8 compliant output

### Integration

#### StubMerger (`_stub_merger.py`)

The `StubMerger` class now uses the IR system internally:

```python
def merge_stub_incremental(self, output_path, new_calls, ...):
    # Parse new calls to IR
    new_stub = self._call_parser.parse_calls(new_calls, ...)

    # Parse existing stub to IR (if exists)
    existing_stub = self._pyi_parser.parse_file(output_path)

    # Merge using IR operations
    for registry_name, new_registry in new_stub.registries.items():
        existing_stub.merge_registry(new_registry, mode=mode)

    # Serialize back to text
    return self._serializer.serialize_to_text(existing_stub)
```

The class maintains backward compatibility by keeping the same public API.

#### TypeStubGenerator (`_generator.py`)

The `generate_stub_content()` method now uses IR-based generation:

```python
def generate_stub_content(self, calls, ...):
    parser = CallInfoToIRParser(self.function_name)
    serializer = IRToTextSerializer(self.function_name)

    stub_ir = parser.parse_calls(calls, ...)
    return serializer.serialize_to_text(stub_ir)
```

## Benefits

### Separation of Concerns

- **Parsing**: Isolated in `_stub_parser.py`, handles AST traversal and extraction
- **Manipulation**: Defined in `_stub_ir.py`, clean operations on structured data
- **Serialization**: Isolated in `_stub_serializer.py`, handles formatting and output

### Improved Maintainability

- No more brittle text-based manipulation
- Clear data structures represent stub file content
- Easy to add new features (e.g., custom decorators, additional metadata)

### Better Testing

- Each component can be tested independently
- Round-trip tests verify correctness: source → IR → text → IR
- Merge operations are testable without file I/O

### Extensibility

- Easy to add new merge strategies
- Simple to implement additional transformations on IR
- Can support multiple output formats (not just .pyi)

## Merge Modes

The IR system supports three merge modes:

### Incremental Mode

Adds new overloads to existing ones:

```python
existing_stub.merge_registry(new_registry, mode="incremental")
```

- Existing overloads are preserved
- New overloads are added
- Duplicates (same query literal) are detected and skipped

### Full Mode

Replaces entire registry:

```python
existing_stub.merge_registry(new_registry, mode="full")
```

- All existing overloads are removed
- New overloads completely replace them
- Useful for complete regeneration

### Prune Mode

Removes stale overloads (only removes, never adds):

```python
existing_stub.merge_registry(new_registry, mode="prune")
```

- Removes overloads no longer present in new registry
- Does NOT add new overloads that weren't already there
- Preserves existing overloads that are still in new registry
- Useful for cleanup after code changes (removing deleted queries)

## Migration from Old System

The refactoring maintains full backward compatibility:

- All existing public APIs are preserved
- Old methods delegate to new IR-based implementation
- Tests continue to pass without modification

## Future Enhancements

The IR system enables several future enhancements:

1. **AST Node References**: Use `asttokens` to maintain precise source locations
2. **Incremental Updates**: Update only specific AST nodes instead of regenerating entire file
3. **Custom Metadata**: Attach comments, source locations, or other metadata to overloads
4. **Multi-format Output**: Generate not just .pyi but also documentation, type stubs for other tools
5. **Advanced Analysis**: Analyze relationships between overloads, detect conflicts
6. **Better Error Messages**: Use IR to provide precise error locations and suggestions

## Code Examples

### Creating IR from Scratch

```python
from sqlty._stub_ir import StubFile, RegistryClass, OverloadSignature

# Create a stub file
stub = StubFile()

# Create a registry
registry = RegistryClass(name="UserQueries", base_class="SQLRegistry")

# Add overloads
registry.add_overload(OverloadSignature(
    query_literal="SELECT id::INTEGER, name::TEXT FROM users",
    return_type="SQL[tuple[int, str]]"
))

# Add to stub
stub.add_registry(registry)

# Serialize to text
from sqlty._stub_serializer import IRToTextSerializer
serializer = IRToTextSerializer()
text = serializer.serialize_to_text(stub)
```

### Parsing Existing Stub

```python
from pathlib import Path
from sqlty._stub_parser import PyiToIRParser

parser = PyiToIRParser()
stub = parser.parse_file(Path("query.pyi"))

# Inspect the IR
for name, registry in stub.registries.items():
    print(f"Registry: {name}")
    for overload in registry.get_sorted_overloads():
        print(f"  - {overload.query_literal[:50]}...")
```

### Merging Stubs

```python
from sqlty._stub_ir import StubFile, RegistryClass, OverloadSignature

# Load existing stub
existing = parser.parse_file(Path("query.pyi"))

# Create new overloads
new_registry = RegistryClass(name="Query", base_class="SQLRegistry")
new_registry.add_overload(OverloadSignature(
    query_literal="SELECT COUNT(*) FROM users",
    return_type="SQL[tuple[int]]"
))

# Merge incrementally
existing.merge_registry(new_registry, mode="incremental")

# Save back
serializer = IRToTextSerializer()
Path("query.pyi").write_text(serializer.serialize_to_text(existing))
```

## Testing

Comprehensive tests are in `tests/ir/test_stub_ir.py`:

- IR data structure operations (add, remove, equality)
- Parsing from CallInfo and .pyi files
- Serialization to AST and text
- Merge operations (incremental, full, prune)
- Round-trip conversions
- Import computation

Run tests:

```bash
pytest tests/ir/ -v
```

## References

- Issue: [Refactor code generation to use intermediate structures](https://github.com/jirikuncar/sqlty/issues/XXX)
- PR: [Implement IR-based code generation](https://github.com/jirikuncar/sqlty/pull/XXX)
