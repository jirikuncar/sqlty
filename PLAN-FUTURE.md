# Future: Stub Generation, Protocol Support, Rust Core

> **Status:** Deferred. These features are not part of the current implementation plan.
> See `PLAN.md` for the active work (mdtest framework, TypeCodec, rewriter/strip, check/lint CLI, CI).

---

## Registries Command

### What it does

Discover and list all `@sqlty`-decorated functions (registries) across the project. Equivalent to the current `python -m sqlty.registries`.

### CLI command (deferred)

```
sqlty registries .      # List discovered registries
```

| Command | What it does | Analogy |
|---------|-------------|---------|
| `sqlty registries .` | List discovered registries | `ruff rule` |

### Dependencies

Depends on the `_collector.py` module (`LiteralCallCollector`) which is part of the stub generation pipeline.

---

## Stub Generation (`.pyi` overload signatures)

### What it does

Analyze Python source files to find calls to `@sqlty`-decorated functions with SQL string literals, infer return types via `SQLAnalyzer`, and generate `.pyi` stub files with `@overload` signatures for each unique query.

### CLI commands (deferred)

```
sqlty generate .           # Analyze source, emit .pyi stubs
sqlty generate --check .   # Exit 1 if stubs are out of date
sqlty clean .              # Delete generated .pyi files
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
├── multi_registry.md        # Multiple registries
├── function_config.md       # Configured functions (no decorator)
└── multiline_queries.md     # Multiline SQL in stubs
```

### Assertion type

`# stub:` with `.pyi` code block — writes files to temp dir, runs `sqlty generate`, compares output.

### Migration from current CLI

The current `sqlty <path>` (no subcommand) maps to `sqlty generate <path>`. For backward compatibility, keep the bare form working but emit a deprecation warning pointing to `sqlty generate`.

---

## Protocol Support

### Design

**Create `docs/type-codecs.md`** — architecture document for future protocol support.

The `TypeCodec` abstraction (in `PLAN.md` Phase 1) is the foundation. Protocol support means:

1. Custom `TypeCodec` implementations that format types as Protocol classes instead of tuples
2. Generated Protocol stubs with named attributes (not just positional tuple elements)
3. Integration with dataclass/attrs/pydantic models

### Example (future)

```python
class UserCodec:
    def format_type(self, column_types: list[str], column_names: list[str]) -> str:
        # Generate a TypedDict or Protocol instead of tuple
        fields = ", ".join(f"{name}={typ}" for name, typ in zip(column_names, column_types))
        return f"TypedDict('Result', {fields})"
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

## Rust Core (optional, high-impact)

### Goal

Replace Python hotpath with Rust via PyO3 + maturin.

### Architecture

**Layer 1: Core** — Pure functions, no I/O. `sqlparser-rs` replaces sqlglot, `TypeCodec` becomes a Rust trait, exposed via PyO3.

**Layer 2: Rewriter** — `ruff_python_parser` + `ruff_python_ast` for Python AST. Biggest perf win.

### Rust mdtest runner

The key benefit of mdtests: the **same `.md` files** test both implementations.

```rust
// Rust mdtest runner (in integration tests)
#[test]
fn test_basic_types() {
    let suite = parse_mdtest("tests/mdtest/type_inference/basic_types.md");
    for case in suite.cases {
        for assertion in case.assertions {
            match assertion {
                Assertion::InferredType { sql, expected } => {
                    let actual = infer_type(&sql, "postgres", "tuple");
                    assert_eq!(actual, expected);
                }
                // ...
            }
        }
    }
}
```

### Crate dependencies

```toml
[dependencies]
pyo3 = { version = "0.22", features = ["extension-module"] }
sqlparser = "0.51"
ruff_python_parser = { git = "https://github.com/astral-sh/ruff" }
ruff_python_ast = { git = "https://github.com/astral-sh/ruff" }
```

### Fallback strategy

```python
try:
    from sqlty._core import rewrite, strip  # Rust (fast)
except ImportError:
    from sqlty._rewriter_py import rewrite, strip  # Pure Python fallback
```

### Rust file structure

| File | Purpose |
|------|---------|
| `Cargo.toml` | Rust workspace config |
| `rust/src/lib.rs` | PyO3 module entry |
| `rust/src/analyzer.rs` | SQL type inference |
| `rust/src/rewriter.rs` | AST rewrite + strip |
| `python/sqlty/_core.pyi` | Type stubs for Rust extension |

### CI for wheels

See `PLAN.md` — the CI section covers building wheels with maturin for multiple platforms. No crate publishing is needed; Rust is only used as a compiled extension inside the Python wheel.
