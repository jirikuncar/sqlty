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

## Structured Return Types (Protocol-based)

### Design

Building on the `return-format` configuration (see `PLAN.md`), the structured types system generates Protocol classes that define the structural contract for query results. Users define their own concrete types (TypedDict, namedtuple, Pydantic models) with human-friendly names that satisfy the generated Protocol.

### How it works

1. **Query analysis**: `SELECT id::INTEGER, name::TEXT FROM users` produces column names + types
2. **Protocol generation**: A `@runtime_checkable Protocol` class is generated with property-style field definitions
3. **User types**: Users define concrete types (TypedDict, dataclass, etc.) that structurally match the Protocol
4. **Type checking**: `SQL[UserType]` is validated against the Protocol — static type checkers verify structural compatibility

### Example

```python
from typing import TypedDict

# User-defined type with a friendly name
class User(TypedDict):
    id: int
    name: str

# sqlty validates that User's fields match the query columns
q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```

The `SQL` class uses a covariant type parameter (`T_co`) so that `SQL[Protocol]` is compatible with `SQL[ConcreteType]` when the concrete type satisfies the protocol:

```python
class SQL(str, Generic[T_co]):
    ...
```

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
