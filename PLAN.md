# Plan: Simplify Snapshot Codec — ruff-style CLI + Rust Core

## Problem Statement

The codebase currently has two largely independent systems for type-safe SQL transformation:

1. **Magic codec** (`_decode.py` + `magic.py`): Token-level transformation at Python import time, intercepting UTF-8 decoding to rewrite `"--sql ..."` strings into `SQL[tuple[...]]()` calls.
2. **Stub generation** (`_generator.py` + `_collector.py` + IR system): AST-level analysis that generates separate `.pyi` stub files with `@overload` signatures.

These share `SQLAnalyzer` for type inference but diverge everywhere else. There is no way to **strip** `SQL[T]` wrappers, no unified CLI structure, and no extension point for protocols.

The goal: restructure sqlty as a **ruff/uv-style multi-command tool** with a clear separation between the fast core (candidate for Rust) and the Python surface.

---

## Proposed CLI Structure (ruff/uv model)

```
sqlty <COMMAND> [OPTIONS] [PATH...]

Commands:
  generate    Generate .pyi stub files with @overload signatures (current default)
  rewrite     Rewrite source files in-place: "--sql ..." → SQL[T]("--sql ...")
  strip       Strip all SQL[T](...) wrappers back to plain strings
  check       Check if files are up-to-date (stubs or rewrites) without modifying
  clean       Remove all generated .pyi stub files
  registries  Discover and list registered @sqlty functions
  version     Print version

Global options:
  --config <PATH>         Path to pyproject.toml or sqlty.toml
  --dialect <DIALECT>     SQL dialect (postgres, sqlite, mysql)
  -v, --verbose           Increase verbosity
  -q, --quiet             Suppress output
```

### Command details

| Command | What it does | Analogy |
|---------|-------------|---------|
| `sqlty generate .` | Analyze source, emit `.pyi` stubs (current `sqlty .`) | `ruff check --fix` |
| `sqlty generate --check .` | Exit 1 if stubs are out of date | `ruff check` |
| `sqlty rewrite .` | Transform `"--sql ..."` → `SQL[T](...)` in source files | `ruff format` |
| `sqlty rewrite --check .` | Exit 1 if source files need rewriting | `ruff format --check` |
| `sqlty rewrite --diff .` | Show diff without applying | `ruff format --diff` |
| `sqlty strip .` | Reverse of rewrite: `SQL[T]("--sql ...")` → `"--sql ..."` | (no ruff analogy — unique to sqlty) |
| `sqlty strip --check .` | Exit 1 if files contain SQL[T] wrappers | — |
| `sqlty check .` | Shorthand: runs both `generate --check` and `rewrite --check` | `ruff check` |
| `sqlty clean .` | Delete generated `.pyi` files | `ruff clean` |
| `sqlty registries .` | List discovered registries (current `python -m sqlty.registries`) | `ruff rule` |

### Migration from current CLI

The current `sqlty <path>` (no subcommand) maps to `sqlty generate <path>`. For backward compatibility, keep the bare form working but emit a deprecation warning pointing to `sqlty generate`.

---

## Current Architecture Analysis

### What works well
- `SQLAnalyzer` (`_sql_analyzer.py`): Solid SQL-to-Python type mapping, shared by both paths
- IR system (`_stub_ir.py`, `_stub_parser.py`, `_stub_serializer.py`): Clean parse → manipulate → serialize pipeline
- Surgical editing via `asttokens` in `IRToTextSerializer._serialize_with_preservation()`
- `inline-snapshot` test pattern in e2e tests

### What needs simplification
- `_decode.py` operates at the **token level** (via `tokenize`) while everything else uses **AST** — fundamental mismatch
- `_decode.py:_infer_sql_type()` duplicates `SQLAnalyzer.generate_return_type()`
- `_decode.py:_transform_sql_string()` does manual quote parsing (lines 93-99) instead of AST — fragile
- The codec intercepts **all UTF-8 decoding** via `.pth` — invasive, conflicts with other tools
- Import injection in `_decode.py` (lines 170-207) is entirely separate from `StubFile.compute_required_imports()`
- `_transform_sql_string()` hardcodes `SQL[tuple[int]]("--sql ...")` at line 110 — no extension point
- **No strip/reverse operation** — once you rewrite, you can't undo
- **Flat CLI** — everything crammed into one argparse with flags (`--check`, `--mode`) instead of subcommands

### Key insight
All operations (generate, rewrite, strip, check) answer variants of the same question: "Given a SQL string, what is its typed representation?" The pipeline is always:
1. **Find** SQL strings in source (`_collector.py` for AST, `_decode.py` for tokens — should be one)
2. **Infer** types via `SQLAnalyzer` (already unified)
3. **Format** the type expression (hardcoded as `tuple[...]` — needs a codec abstraction)
4. **Emit** into target (stub file, rewritten source, or stripped source — needs the subcommand split)

---

## Proposed Architecture

### Layer 1: Core (Rust-candidate)

These are pure functions with no I/O, no Python imports, no side effects. They are the hotpath and the natural boundary for a Rust rewrite:

```
┌──────────────────────────────────────────────────────┐
│  sqlty-core (pure computation, no I/O)               │
│                                                      │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────┐  │
│  │ SQL Parser   │  │ Type Mapper  │  │ Type Codec │  │
│  │ (sqlglot)    │  │ SQL→Python   │  │ tuple/proto│  │
│  └──────┬──────┘  └──────┬───────┘  └─────┬──────┘  │
│         └────────────┬───┘                 │         │
│                ┌─────▼─────┐               │         │
│                │ Analyzer   │◄──────────────┘         │
│                │ infer_type │                         │
│                └────────────┘                         │
└──────────────────────────────────────────────────────┘
```

**Current files in this layer:**
- `_sql_analyzer.py` — SQL parsing + type inference (wraps sqlglot)
- `_type_codec.py` — **new**: pluggable type formatting

**Rust migration path:** Replace sqlglot with `sqlparser-rs`, type mapping becomes a Rust `HashMap`, `TypeCodec` becomes a Rust trait. Expose via PyO3 as a single `infer_type(sql: str, codec: str) -> str` function.

### Layer 2: Rewriter (Rust-candidate)

AST-level source transformation. Finds SQL strings, calls Layer 1 for type inference, produces transformed source:

```
┌───────────────────────────────────────────────────────┐
│  sqlty-rewriter (source → source transformation)      │
│                                                       │
│  ┌─────────────┐   ┌─────────────┐   ┌────────────┐  │
│  │ AST Finder   │   │ Rewrite     │   │ Strip      │  │
│  │ find --sql   │   │ str→SQL[T]  │   │ SQL[T]→str │  │
│  │ strings      │   │ + imports   │   │ - imports   │  │
│  └──────┬──────┘   └──────┬──────┘   └─────┬──────┘  │
│         └────────────┬────┘                 │         │
│                ┌─────▼──────┐               │         │
│                │ SQLRewriter │◄──────────────┘         │
│                │ .rewrite() │                         │
│                │ .strip()   │                         │
│                └────────────┘                         │
└───────────────────────────────────────────────────────┘
```

**Current files (to be refactored):**
- `_decode.py` → replaced by `_rewriter.py`
- `magic.py` → thin wrapper calling `_rewriter.rewrite()`

**Rust migration path:** Use `ruff_python_parser` (from ruff's crate) for AST parsing, `ruff_python_ast` for node types. The rewriter becomes Rust operating on ruff's AST. This is where the biggest performance win is — ruff's parser is 10-100x faster than Python's `ast` module.

### Layer 3: IR + Stub Generation (Python)

The existing IR system for `.pyi` stub generation. This stays in Python because it's I/O-heavy and not the bottleneck:

```
┌────────────────────────────────────────────────┐
│  sqlty-stubs (IR-based stub generation)        │
│                                                │
│  _collector.py → _stub_parser.py → _stub_ir.py │
│                                    ↓           │
│                         _stub_serializer.py    │
│                                    ↓           │
│                          _stub_merger.py       │
│                                    ↓           │
│                          _generator.py         │
└────────────────────────────────────────────────┘
```

Unchanged. Calls Layer 1 for type inference.

### Layer 4: CLI (Python, thin)

The ruff-style subcommand router. Pure dispatch, no business logic:

```
┌─────────────────────────────────────────────────┐
│  sqlty CLI (ruff-style subcommands)             │
│                                                 │
│  sqlty generate .  → Layer 3 (_generator.py)    │
│  sqlty rewrite .   → Layer 2 (_rewriter.py)     │
│  sqlty strip .     → Layer 2 (_rewriter.py)     │
│  sqlty check .     → Layer 2 + 3 (check mode)   │
│  sqlty clean .     → file deletion              │
│  sqlty registries  → _registry.py               │
└─────────────────────────────────────────────────┘
```

---

## Implementation Plan

### Phase 1: TypeCodec abstraction

**Create `src/sqlty/_type_codec.py`** (~30 lines):

```python
from typing import Protocol

class TypeCodec(Protocol):
    def format_type(self, column_types: list[str]) -> str: ...

class TupleCodec:
    def format_type(self, column_types: list[str]) -> str:
        return f"tuple[{', '.join(column_types)}]"
```

**Modify `src/sqlty/_sql_analyzer.py:316-340`:**
- `generate_return_type()` accepts optional `TypeCodec` (default `TupleCodec()`)
- Replaces hardcoded `f"tuple[{', '.join(types)}]"` with `codec.format_type(types)`
- All existing callers unchanged (default parameter)

**Tests:** `tests/test_type_codec.py` — TupleCodec with 0/1/N columns, pluggable custom codec

### Phase 2: Unified rewriter with strip support

**Create `src/sqlty/_rewriter.py`** (~150 lines):

```python
class SQLRewriter:
    """AST-based source rewriter. Shared by codec, rewrite CLI, and strip CLI."""

    def __init__(self, codec: TypeCodec | None = None, analyzer: SQLAnalyzer | None = None):
        self.codec = codec or TupleCodec()
        self.analyzer = analyzer or SQLAnalyzer()

    def rewrite(self, source: str) -> str:
        """Transform "--sql ..." strings → SQL[T]("--sql ...") + add imports."""
        ...

    def strip(self, source: str) -> str:
        """Reverse: SQL[T]("--sql ...") → "--sql ..." + remove unused imports."""
        ...
```

The `strip()` method is the key new capability:
- Finds all `SQL[T](string_literal)` call expressions via AST
- Replaces each with just `string_literal`
- Removes `from sqlty import SQL` if no longer needed
- Removes `import datetime`/`import uuid` if no longer referenced

**Simplify `src/sqlty/_decode.py`:**
- `decode()` → thin wrapper: `SQLRewriter().rewrite(source)`
- Delete `_infer_sql_type()`, `_transform_sql_string()`, token loop, import injection
- Keep function signature for codec compat

**Tests:**
- `tests/test_rewriter.py`: rewrite and strip unit tests, round-trip (`rewrite(strip(x)) == x`), idempotency
- Existing `tests/test_magic.py` and `tests/e2e/test_magic_codec_e2e.py` pass unchanged

### Phase 3: ruff-style CLI restructure

**Rewrite `src/sqlty/_cli.py`** as subcommand router:

```python
def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="sqlty")
    subparsers = parser.add_subparsers(dest="command")

    # sqlty generate
    gen = subparsers.add_parser("generate", help="Generate .pyi stub files")
    gen.add_argument("source", type=Path)
    gen.add_argument("--check", action="store_true")
    gen.add_argument("--mode", type=str)
    gen.add_argument("-o", "--output", type=Path)
    gen.add_argument("--schema", action="append")
    gen.add_argument("--format-command", type=str)

    # sqlty rewrite
    rw = subparsers.add_parser("rewrite", help="Rewrite --sql strings to SQL[T](...)")
    rw.add_argument("source", type=Path, nargs="*", default=[Path(".")])
    rw.add_argument("--check", action="store_true")
    rw.add_argument("--diff", action="store_true")

    # sqlty strip
    st = subparsers.add_parser("strip", help="Strip SQL[T](...) back to plain strings")
    st.add_argument("source", type=Path, nargs="*", default=[Path(".")])
    st.add_argument("--check", action="store_true")
    st.add_argument("--diff", action="store_true")

    # sqlty check (convenience: runs generate --check + rewrite --check)
    chk = subparsers.add_parser("check", help="Check stubs and rewrites are up-to-date")
    chk.add_argument("source", type=Path, nargs="*", default=[Path(".")])

    # sqlty clean
    cln = subparsers.add_parser("clean", help="Remove generated .pyi files")
    cln.add_argument("source", type=Path, nargs="*", default=[Path(".")])

    # sqlty registries
    reg = subparsers.add_parser("registries", help="List registered @sqlty functions")
    reg.add_argument("source", type=Path, nargs="?", default=Path("."))

    # Global options
    for p in [parser, gen, rw, st, chk, cln, reg]:
        p.add_argument("--dialect", default="postgres")
        p.add_argument("-v", "--verbose", action="count", default=0)
        p.add_argument("-q", "--quiet", action="store_true")

    # Backward compat: bare "sqlty <path>" without subcommand → generate
    ...
```

**Move `registries/_cli.py` logic** into the main CLI under `sqlty registries`.

**Update `src/sqlty/__main__.py`** — no changes needed (already calls `_cli.main()`).

**Tests:**
- `tests/e2e/test_rewrite_cli.py`: rewrite + check + diff + idempotency
- `tests/e2e/test_strip_cli.py`: strip + check + round-trip with rewrite
- `tests/e2e/test_cli_compat.py`: bare `sqlty .` still works (deprecation warning)
- All existing e2e tests pass (they use subprocess to call `sqlty`)

### Phase 4: Protocol support (design only)

**Create `docs/type-codecs.md`** — architecture document for future protocol support:

```python
# Future: ProtocolCodec
class ProtocolCodec:
    def format_type(self, column_types: list[str], column_names: list[str]) -> str:
        # Generates:
        #   class UserRow(Protocol):
        #       id: int
        #       name: str
        # Returns "UserRow"
        ...
```

**What changes for protocol support:**
- `TypeCodec.format_type()` gains `column_names` parameter (from `ColumnInfo.name`)
- `SQLAnalyzer.analyze_select()` already returns `ColumnInfo(name, python_type, sql_type)` — data is there
- Rewriter emits Protocol class definitions (inline or separate file)
- Config: `[tool.sqlty] type_codec = "tuple"` vs `"protocol"`

### Phase 5: Rust core (optional, high-impact)

**Goal:** Replace the Python hotpath with Rust, linked as a native extension via PyO3 + maturin (same pattern as pydantic-core).

#### Build setup

```toml
# pyproject.toml changes
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[tool.maturin]
features = ["pyo3/extension-module"]
python-source = "python"
module-name = "sqlty._core"
```

#### Project layout

```
sqlty/
├── Cargo.toml                    # Rust workspace
├── pyproject.toml                # Python package (maturin build)
├── src/                          # Rust source
│   ├── lib.rs                    # PyO3 module entry point
│   ├── analyzer.rs               # SQL parsing + type inference
│   ├── type_codec.rs             # TypeCodec trait + TupleCodec
│   └── rewriter.rs               # AST-based rewrite + strip
├── python/
│   └── sqlty/                    # Python source (was src/sqlty/)
│       ├── __init__.py           # SQL[T] class, @sqlty decorator
│       ├── _core.pyi             # Type stubs for Rust extension
│       ├── _cli.py               # CLI (pure Python, calls _core)
│       ├── _generator.py         # Stub generation (pure Python)
│       ├── _stub_ir.py           # IR system (pure Python)
│       ├── _stub_parser.py       # IR parsers (pure Python)
│       ├── _stub_serializer.py   # IR serializers (pure Python)
│       ├── magic.py              # Codec (calls _core.rewrite)
│       └── ...
└── tests/
```

#### What moves to Rust

| Component | Current Python | Rust replacement | Crate dependency |
|-----------|---------------|-----------------|------------------|
| SQL parsing | `sqlglot` | `sqlparser-rs` | `sqlparser = "0.51"` |
| Type mapping | `SQLAnalyzer.SQL_TO_PYTHON_TYPE` dict | `HashMap<&str, &str>` | (none) |
| Type inference | `SQLAnalyzer.generate_return_type()` | `fn infer_type(sql, codec) -> String` | (none) |
| AST parsing | `ast.parse()` + `asttokens` | `ruff_python_parser` | `ruff_python_parser` |
| AST types | `ast.FunctionDef`, etc. | `ruff_python_ast` | `ruff_python_ast` |
| Rewrite | `_rewriter.py` (Phase 2) | `fn rewrite(source) -> String` | ruff crates |
| Strip | `_rewriter.py` (Phase 2) | `fn strip(source) -> String` | ruff crates |

#### What stays in Python

| Component | Reason |
|-----------|--------|
| CLI (`_cli.py`) | Thin dispatch, not performance-critical |
| IR system (`_stub_ir.py`, `_stub_parser.py`, `_stub_serializer.py`) | Complex AST manipulation, I/O-heavy |
| Stub generation (`_generator.py`, `_collector.py`) | File system operations |
| Magic codec (`magic.py`) | Python codec API requirement |
| Config (`_config.py`) | TOML parsing, I/O |

#### Rust crate dependencies

```toml
# Cargo.toml
[dependencies]
pyo3 = { version = "0.22", features = ["extension-module"] }
sqlparser = "0.51"         # SQL parsing (replaces sqlglot)

# From ruff's workspace — use as git dependencies or publish separately
ruff_python_parser = { git = "https://github.com/astral-sh/ruff" }
ruff_python_ast = { git = "https://github.com/astral-sh/ruff" }
```

#### PyO3 interface

```rust
// src/lib.rs
use pyo3::prelude::*;

#[pyfunction]
fn infer_type(sql: &str, dialect: &str, codec: &str) -> PyResult<String> {
    // Parse SQL, extract columns, format type
    ...
}

#[pyfunction]
fn rewrite(source: &str, dialect: &str) -> PyResult<String> {
    // Find --sql strings, infer types, produce rewritten source
    ...
}

#[pyfunction]
fn strip(source: &str) -> PyResult<String> {
    // Find SQL[T](...) calls, unwrap to plain strings
    ...
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(infer_type, m)?)?;
    m.add_function(wrap_pyfunction!(rewrite, m)?)?;
    m.add_function(wrap_pyfunction!(strip, m)?)?;
    Ok(())
}
```

#### Fallback strategy

```python
# python/sqlty/_rewriter.py
try:
    from sqlty._core import rewrite, strip  # Rust (fast)
except ImportError:
    from sqlty._rewriter_py import rewrite, strip  # Pure Python fallback
```

This ensures the package works without Rust compilation (source installs, unsupported platforms) while providing 10-100x speedup when the native extension is available.

---

## File Change Summary

### Phase 1-3 (Python only)

#### New files
| File | Phase | Purpose |
|------|-------|---------|
| `src/sqlty/_type_codec.py` | 1 | TypeCodec Protocol + TupleCodec |
| `src/sqlty/_rewriter.py` | 2 | AST-based rewrite + strip engine |
| `tests/test_type_codec.py` | 1 | TypeCodec unit tests |
| `tests/test_rewriter.py` | 2 | Rewriter + strip unit tests |
| `tests/e2e/test_rewrite_cli.py` | 3 | `sqlty rewrite` e2e tests |
| `tests/e2e/test_strip_cli.py` | 3 | `sqlty strip` e2e tests |
| `docs/type-codecs.md` | 4 | Type codec architecture doc |

#### Modified files
| File | Phase | Change |
|------|-------|--------|
| `src/sqlty/_sql_analyzer.py` | 1 | Accept TypeCodec in `generate_return_type()` |
| `src/sqlty/_decode.py` | 2 | Simplify to thin wrapper over `SQLRewriter.rewrite()` |
| `src/sqlty/_cli.py` | 3 | Rewrite as ruff-style subcommand router |
| `src/sqlty/registries/_cli.py` | 3 | Absorb into main CLI `sqlty registries` |

#### Unchanged files
| File | Reason |
|------|--------|
| `src/sqlty/magic.py` | Already delegates to `_decode.decode()` |
| `src/sqlty/__init__.py` | `SQL[T]` class unchanged |
| `src/sqlty/_stub_ir.py` | IR system unchanged |
| `src/sqlty/_stub_parser.py` | Parsing unchanged |
| `src/sqlty/_stub_serializer.py` | Serialization unchanged |
| `src/sqlty/_stub_merger.py` | Merging unchanged |
| `src/sqlty/_generator.py` | Stub generation unchanged |
| `src/sqlty/_collector.py` | Collection unchanged |

### Phase 5 (Rust core)

#### New files
| File | Purpose |
|------|---------|
| `Cargo.toml` | Rust workspace config |
| `src/lib.rs` | PyO3 module entry point |
| `src/analyzer.rs` | SQL parsing + type inference |
| `src/type_codec.rs` | TypeCodec trait + TupleCodec |
| `src/rewriter.rs` | AST rewrite + strip |
| `python/sqlty/_core.pyi` | Type stubs for Rust extension |

#### Modified files
| File | Change |
|------|--------|
| `pyproject.toml` | Switch build-backend to maturin |
| `src/sqlty/` → `python/sqlty/` | Relocate Python source for maturin layout |
| `python/sqlty/_rewriter.py` | Try Rust import, fallback to pure Python |

---

## Testing Strategy

### Unit tests
- `test_type_codec.py`: TupleCodec with 0/1/N columns; custom codec
- `test_rewriter.py`: rewrite, strip, round-trip (`rewrite ∘ strip = id`), idempotency, import management

### Integration tests
- Existing `test_magic.py` — behavioral parity after `_decode.py` simplification
- `test_rewriter.py` — rewriter produces identical output to old `_decode.py`

### E2E tests
- `test_rewrite_cli.py`: `sqlty rewrite .`, `--check`, `--diff`, idempotency, mypy validation
- `test_strip_cli.py`: `sqlty strip .`, `--check`, round-trip with rewrite
- `test_cli_compat.py`: bare `sqlty <path>` triggers deprecation + works
- All existing e2e tests pass unchanged

### Round-trip invariants
```
strip(rewrite(source)) == source           # for any source with --sql strings
rewrite(rewrite(source)) == rewrite(source) # idempotent
strip(strip(source)) == strip(source)       # idempotent
```

### Rust parity tests (Phase 5)
- Same test corpus run against both Python and Rust implementations
- Differential testing: `assert rust_rewrite(s) == python_rewrite(s)` for all fixtures

---

## Key Design Decisions

1. **ruff-style subcommands** — `generate`, `rewrite`, `strip`, `check`, `clean`, `registries` instead of flat flags. Each command does one thing well.
2. **`strip` as a first-class command** — reversibility is a feature. `sqlty strip .` undoes all `SQL[T]` wrappers in a single command, returning to plain `"--sql ..."` strings.
3. **TypeCodec is a Protocol** — duck typing, no inheritance, future codecs don't import sqlty.
4. **Rewriter uses AST (not tokens)** — consistent with codebase, robust, enables Protocol class emission.
5. **Rust core is optional** — pure Python fallback ensures the package always works. Rust is a performance optimization, not a requirement.
6. **ruff crates for Python AST** — don't reinvent the parser. `ruff_python_parser` + `ruff_python_ast` are battle-tested, fast, and maintained.
7. **`sqlparser-rs` for SQL** — replaces sqlglot in Rust path. Well-maintained, supports postgres/mysql/sqlite dialects.
8. **maturin for build** — same build tool as pydantic-core. Handles PyO3 compilation, wheel building, cross-platform binaries.
9. **Backward compat** — bare `sqlty <path>` still works as `sqlty generate <path>` with deprecation warning.
