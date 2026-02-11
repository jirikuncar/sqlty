# Future: Stub Generation, Structured Types

> **Status:** Deferred. These features are not part of the current implementation plan.
> See `PLAN.md` for the active work (mdtest framework, TypeCodec, rewriter/strip, check/lint CLI, Rust core, CI).

---

## Stub Generation (`.pyi` overload signatures)

### What it does

Analyze Python source files to find calls to configured functions with SQL string literals, infer return types via `SQLAnalyzer`, and generate `.pyi` stub files with `@overload` signatures for each unique query. Functions are registered via `[[tool.sqlty.functions]]` in `pyproject.toml` — no decorator needed.

### CLI commands (deferred)

```
sqlty generate .           # Analyze source, emit .pyi stubs
sqlty generate --check .   # Exit 1 if stubs are out of date
sqlty clean .              # Delete generated .pyi files
```

### Configuration

Functions are registered in `pyproject.toml` using the standard `[tool.sqlty]` convention:

```toml
[[tool.sqlty.functions]]
import_path = "db.query"
query_param = "sql"
return_type_param = "return_tp"
schema = "db/schema.sql"
return_type_template = "list[{sqlty}]"
imports = ["from typing import Any"]
```

### Current implementation

The existing codebase already has a full stub generation pipeline:

| File | Purpose |
|------|---------|
| `src/sqlty/_generator.py` | Main stub generation orchestrator |
| `src/sqlty/_collector.py` | `LiteralCallCollector` — AST visitor finding SQL string args |
| `src/sqlty/_stub_ir.py` | IR: `OverloadSignature`, `RegistryDecorator`, `ImportStatement`, `StubFile` |
| `src/sqlty/_stub_parser.py` | `PyiToIRParser`, `CallInfoToIRParser` |
| `src/sqlty/_stub_serializer.py` | `IRToASTSerializer`, `IRToTextSerializer` |
| `src/sqlty/_stub_merger.py` | Merge modes: incremental, prune, full |
| `src/sqlty/_auto_generator.py` | Runtime stub generation |

### Merge modes

- **Incremental:** Add new overloads, keep existing ones
- **Prune:** Remove overloads for queries no longer in source
- **Full:** Regenerate from scratch

### mdtest files (deferred)

```
tests/mdtest/generate/
├── basic_stubs.md           # Single query → .pyi
├── incremental_merge.md     # Add queries to existing stubs
├── prune.md                 # Remove stale queries
├── function_config.md       # Configured functions
└── multiline_queries.md     # Multiline SQL in stubs
```

### Assertion type

`# stub:` with `.pyi` code block — writes files to temp dir, runs `sqlty generate`, compares output.

---

## User-Defined Type Checking (`SQL[UserType]`)

> **Status:** Implemented (Python backend). Validates user-defined types structurally against SQL query columns.

### Overview

When `check_source` encounters `SQL[UserType]("--sql\n...")`, it extracts the type definition from the source file and validates its fields against the SQL query columns. This is a **structural check** — it validates field count, types, and names — as opposed to the string-based `stale-type` check used for `tuple[...]` annotations.

### Supported type forms

| Type Form | Detection | Example |
|-----------|-----------|---------|
| `TypedDict` | Base class `TypedDict` | `class User(TypedDict): id: int; name: str` |
| `NamedTuple` | Base class `NamedTuple` | `class User(NamedTuple): id: int; name: str` |
| `dataclass` | `@dataclass` decorator | `@dataclass class User: id: int; name: str` |
| `BaseModel` | Base class `BaseModel` (Pydantic) | `class User(BaseModel): id: int; name: str` |

### How it works

1. **Source parsing**: `_type_extractor.py` uses Python's `ast` module to find type definitions in the source file
2. **Field extraction**: Annotated assignments (`field: type`) are extracted with type normalization (e.g., `int | None` → `int`)
3. **Structural validation**: The `Checker` class runs `column-count-mismatch`, `column-type-mismatch`, and `column-name-mismatch` checks
4. **stale-type skipped**: User-defined types bypass the `stale-type` rule (which would always fire since `"User" != "tuple[int, str]"`)

### Example

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: int
    name: str

# All checks pass — fields match query columns
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

# column-type-mismatch: User.id is str but query column 'id' infers int
class BadUser(TypedDict):
    id: str  # wrong!
    name: str

q = SQL[BadUser]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

### Implementation files

| File | Purpose |
|------|---------|
| `src/sqlty/_type_extractor.py` | AST-based type definition extraction |
| `src/sqlty/_core_py.py` | Integration with `check_source` |
| `src/sqlty/_checker.py` | Structural validation (column count/type/name) |
| `tests/test_type_extractor.py` | Unit tests for type extraction |
| `tests/test_core_conformance.py` | Integration tests (`TestCheckSourceUserTypes`) |
| `tests/mdtest/check/user_types.md` | Snapshot-based mdtests |

### Limitations

- **Same-file only**: Type definitions must be in the same file as the `SQL[T](...)` usage
- **Python backend only**: Rust `check_source` does not yet perform user-type extraction (planned)
- **No cross-module resolution**: Imported types from other modules are silently skipped
- **No runtime Protocol**: Types are checked statically via AST, not via `@runtime_checkable Protocol`

---

## Structured Return Types (Protocol-based, future)

### Design

Building on the user-defined type checking above, the structured types system would generate Protocol classes that define the structural contract for query results. Users define their own concrete types (TypedDict, namedtuple, Pydantic models) with human-friendly names that satisfy the generated Protocol.

### Future enhancements

1. **Cross-module type resolution**: Follow imports to resolve types defined in other modules
2. **Protocol generation**: Generate `@runtime_checkable Protocol` classes from queries
3. **Covariant SQL class**: `class SQL(str, Generic[T_co])` for Protocol compatibility
4. **Rewrite with user types**: `sqlty rewrite` could generate `SQL[User]` when a matching type exists

### Typed JSON with json_build_object (future)

When `json_build_object` has literal string keys and typed value expressions, sqlty could infer a `TypedDict` instead of plain `dict`:

```sql
SELECT json_build_object('id', id::INTEGER, 'name', name::TEXT)
```
```python
# Future: inferred: tuple[UserJson]
# where UserJson = TypedDict('UserJson', id=int, name=str)
```

This depends on TypeCodec supporting complex return types.

---

## Rust Core (MOVED to PLAN.md Phase 5)

> **Status:** Implemented. See `PLAN.md` Phase 5 for the current Rust extension architecture.
