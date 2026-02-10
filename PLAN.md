# Plan: Simplify Snapshot Codec — ruff-style CLI + Markdown Tests + Rust Core

## Problem Statement

The codebase currently has two largely independent systems for type-safe SQL transformation:

1. **Magic codec** (`_decode.py` + `magic.py`): Token-level transformation at Python import time, intercepting UTF-8 decoding to rewrite `"--sql ..."` strings into `SQL[tuple[...]]()` calls.
2. **Stub generation** (`_generator.py` + `_collector.py` + IR system): AST-level analysis that generates separate `.pyi` stub files with `@overload` signatures.

These share `SQLAnalyzer` for type inference but diverge everywhere else. There is no way to **strip** `SQL[T]` wrappers, no unified CLI structure, no extension point for protocols, and the test suite is fragmented across 30+ pytest files with no shared specification of expected behavior.

The goal: restructure sqlty as a **ruff/uv-style multi-command tool** with a **ty-style markdown test suite** and a clear separation between the fast core (candidate for Rust) and the Python surface.

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

## Markdown Test Framework (mdtest)

Inspired by [ty's mdtest framework](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md) ([original PR](https://github.com/astral-sh/ruff/pull/13636)), sqlty's test suite will be written as markdown files with embedded code blocks and assertion comments. Each `.md` file is a test suite; each `##` header is a test case.

### Why markdown tests

The current test suite has **30+ pytest files** with boilerplate setup code repeated everywhere. Tests for the same behavior (e.g., "integer type inference") are scattered across `test_analyzer.py`, `test_magic.py`, `test_magic_codec_e2e.py`, and `test_basic_workflow.py`. There is no single document that specifies all of sqlty's behavior.

Markdown tests solve this:
- **Single source of truth**: One `.md` file per feature, readable as documentation
- **No boilerplate**: No fixture setup, no `tmp_path`, no subprocess wrangling
- **Shared by Python and Rust**: Same `.md` files test both implementations
- **Discoverable**: Browse `tests/mdtest/` to understand what sqlty does
- **Easy to contribute**: Add a test case = add a markdown section

### Test format specification

A markdown test file is a suite of tests. Each heading creates a named test case. Code blocks contain Python source, SQL queries, or expected outputs. Assertion comments embedded in code blocks declare expected behavior.

#### Assertion types

**`# inferred: <type>`** — asserts the inferred Python type for a SQL query:

```markdown
## Simple integer cast

​```sql
SELECT id::INTEGER FROM users
​```

​```python
# inferred: tuple[int]
​```
```

**`# rewritten: <source>`** — asserts the rewritten form of a `--sql` string:

```markdown
## Rewrite basic query

​```python
q = "--sql SELECT id::INTEGER FROM users"
# rewritten: q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
​```
```

**`# stripped: <source>`** — asserts the stripped (reverse) form:

```markdown
## Strip SQL wrapper

​```python
q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
# stripped: q = "--sql SELECT id::INTEGER FROM users"
​```
```

**`# stub:`** — asserts generated stub content (multiline, delimited by blank line or next assertion):

```markdown
## Generate stub for single query

​```python
from queries import sql
query = sql("SELECT id::INTEGER FROM users")
​```

​```pyi
# stub:
from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal['SELECT id::INTEGER FROM users']) -> SQL[tuple[int]]:
    ...

@overload
def sql(query: str) -> str:
    ...
​```
```

**`# error: <message>`** — asserts an error is raised:

```markdown
## Invalid SQL produces Any

​```sql
NOT VALID SQL QUERY
​```

​```python
# inferred: Any
​```
```

**`# imports:`** — asserts which imports are added:

```markdown
## Datetime imports

​```python
q = "--sql SELECT created_at::TIMESTAMP, user_id::UUID"
# imports: datetime, uuid
# rewritten: q = SQL[tuple[datetime.datetime, uuid.UUID]]("--sql SELECT created_at::TIMESTAMP, user_id::UUID")
​```
```

#### Configuration blocks

TOML blocks configure per-test settings:

```markdown
## MySQL dialect support

​```toml
[sqlty]
dialect = "mysql"
​```

​```sql
SELECT CAST(id AS SIGNED) FROM users
​```

​```python
# inferred: tuple[int]
​```
```

#### Multi-file tests

Explicit file paths declared before code blocks (same as ty's mdtest):

```markdown
## Multi-registry stub generation

`queries.py`:
​```python
from sqlty import sqlty
from typing import Any

@sqlty
def sql(query: str) -> Any:
    return query
​```

`app.py`:
​```python
from queries import sql
q1 = sql("SELECT id::INTEGER FROM users")
q2 = sql("SELECT name::TEXT FROM users")
​```

`pyproject.toml`:
​```toml
[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "queries.sql"
query_param = "query"
​```

`queries.pyi`:
​```pyi
# stub:
from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal['SELECT id::INTEGER FROM users']) -> SQL[tuple[int]]:
    ...

@overload
def sql(query: Literal['SELECT name::TEXT FROM users']) -> SQL[tuple[str]]:
    ...

@overload
def sql(query: str) -> str:
    ...
​```
```

#### Schema-aware tests

```markdown
## Schema-based type inference

`schema.sql`:
​```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```sql
SELECT id, name, created_at FROM users
​```

​```python
# inferred: tuple[int, str, datetime.datetime]
​```
```

#### Round-trip tests

```markdown
## Rewrite then strip is identity

​```python
original = "--sql SELECT id::INTEGER, name::TEXT FROM users"
​```

​```python
# rewritten: original = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# roundtrip: strip(rewrite(source)) == source
​```
```

### Test file organization

```
tests/mdtest/
├── README.md                           # Framework specification (this section)
├── type_inference/
│   ├── basic_types.md                  # int, str, float, bool
│   ├── datetime_types.md               # date, datetime, time, timedelta
│   ├── uuid_types.md                   # uuid.UUID
│   ├── aggregate_functions.md          # COUNT, SUM, AVG
│   ├── schema_inference.md             # Schema-based type inference
│   └── dialects.md                     # postgres, mysql, sqlite differences
├── rewrite/
│   ├── basic_rewrite.md                # "--sql ..." → SQL[T](...)
│   ├── multiline_queries.md            # Triple-quoted strings
│   ├── import_injection.md             # datetime, uuid, SQL import management
│   ├── idempotency.md                  # rewrite(rewrite(x)) == rewrite(x)
│   └── edge_cases.md                   # Nested strings, f-strings, no-op
├── strip/
│   ├── basic_strip.md                  # SQL[T]("--sql ...") → "--sql ..."
│   ├── import_removal.md               # Remove unused imports
│   ├── idempotency.md                  # strip(strip(x)) == strip(x)
│   └── round_trip.md                   # strip(rewrite(x)) == x
├── generate/
│   ├── basic_stubs.md                  # Single query → .pyi
│   ├── incremental_merge.md            # Add queries to existing stubs
│   ├── prune.md                        # Remove stale queries
│   ├── multi_registry.md               # Multiple registries
│   ├── function_config.md              # Configured functions (no decorator)
│   └── multiline_queries.md            # Multiline SQL in stubs
├── codec/
│   ├── basic_codec.md                  # # coding: sqlty transformation
│   ├── auto_imports.md                 # Import injection via codec
│   └── runtime_execution.md            # Transformed code runs correctly
└── type_codec/
    ├── tuple_codec.md                  # Default tuple[T1, T2] formatting
    └── custom_codec.md                 # Pluggable codec interface
```

### Test runner implementation

**`tests/conftest.py`** — pytest plugin that discovers and runs `.md` files:

```python
# tests/conftest.py additions
import pytest
from pathlib import Path
from tests.mdtest_runner import collect_mdtest_file

def pytest_collect_file(parent, file_path):
    if file_path.suffix == ".md" and "mdtest" in str(file_path):
        return MDTestFile.from_parent(parent, path=file_path)

class MDTestFile(pytest.File):
    def collect(self):
        yield from collect_mdtest_file(self.path)

class MDTestItem(pytest.Item):
    def __init__(self, name, parent, test_case):
        super().__init__(name, parent)
        self.test_case = test_case

    def runtest(self):
        self.test_case.run()

    def repr_failure(self, excinfo):
        return f"mdtest failed: {self.test_case.name}\n{excinfo.value}"

    def reportinfo(self):
        return self.path, None, f"mdtest: {self.test_case.name}"
```

**`tests/mdtest_runner.py`** — the core markdown parser and assertion engine (~300 lines):

```python
class MDTestCase:
    """A single test case extracted from a markdown heading."""
    name: str
    config: dict          # From ```toml blocks
    files: dict[str, str] # path → content from named code blocks
    assertions: list[Assertion]

    def run(self):
        """Execute all assertions in this test case."""
        for assertion in self.assertions:
            match assertion:
                case InferredType(sql=sql, expected=expected):
                    analyzer = SQLAnalyzer(**self._analyzer_kwargs())
                    actual = analyzer.generate_return_type(sql)
                    assert actual == expected, f"Expected {expected}, got {actual}"

                case Rewritten(source=source, expected=expected):
                    rewriter = SQLRewriter(**self._rewriter_kwargs())
                    actual = rewriter.rewrite(source)
                    assert normalize(actual) == normalize(expected)

                case Stripped(source=source, expected=expected):
                    rewriter = SQLRewriter(**self._rewriter_kwargs())
                    actual = rewriter.strip(source)
                    assert normalize(actual) == normalize(expected)

                case StubContent(source_files=files, expected_stub=stub):
                    # Write files to tmp, run generator, compare
                    ...

                case RoundTrip(source=source):
                    rewriter = SQLRewriter(**self._rewriter_kwargs())
                    assert rewriter.strip(rewriter.rewrite(source)) == source
```

**Key design properties:**
- Zero boilerplate: no fixtures, no tmp_path, no subprocess calls
- Each assertion type maps directly to one sqlty operation
- Config blocks propagate to all assertions in the same section
- Multi-file tests use an in-memory filesystem (no disk I/O for unit tests)
- The same `.md` files work for both Python and Rust test runners

### Running mdtests

```bash
# Run all mdtests
pytest tests/mdtest/ -v

# Run a specific mdtest file
pytest tests/mdtest/type_inference/basic_types.md -v

# Filter to a specific test case (uses pytest -k)
pytest tests/mdtest/ -k "multiple columns"

# Run with environment variable filter (ty-compatible)
MDTEST_FILTER="integer cast" pytest tests/mdtest/ -v
```

### Migration from existing tests

The existing pytest files are **not deleted** during migration. Instead:

1. For each existing test, write the equivalent mdtest in the appropriate `.md` file
2. Mark the old test with `@pytest.mark.legacy`
3. Run both suites in CI until confident in parity
4. Delete legacy tests once mdtest coverage is complete

The key mappings from existing tests:

| Existing file | → mdtest file |
|--------------|---------------|
| `tests/sql_analyzer/test_analyzer.py` | `tests/mdtest/type_inference/basic_types.md` |
| `tests/sql_analyzer/test_functions.py` | `tests/mdtest/type_inference/aggregate_functions.md` |
| `tests/sql_analyzer/test_returning_clause.py` | `tests/mdtest/type_inference/basic_types.md` (RETURNING section) |
| `tests/sql_analyzer/test_sum_schema.py` | `tests/mdtest/type_inference/schema_inference.md` |
| `tests/test_magic.py` | `tests/mdtest/rewrite/basic_rewrite.md` |
| `tests/test_magic_integration.py` | `tests/mdtest/codec/basic_codec.md` |
| `tests/e2e/test_magic_codec_e2e.py` | `tests/mdtest/codec/basic_codec.md` |
| `tests/e2e/test_basic_workflow.py` | `tests/mdtest/generate/basic_stubs.md` |
| `tests/e2e/test_multi_registry.py` | `tests/mdtest/generate/multi_registry.md` |
| `tests/e2e/test_dialects_and_schema.py` | `tests/mdtest/type_inference/dialects.md` |
| `tests/e2e/test_cli_modes.py` | `tests/mdtest/generate/incremental_merge.md` + `prune.md` |
| `tests/e2e/test_function_config.py` | `tests/mdtest/generate/function_config.md` |
| `tests/ir/test_stub_ir.py` | (stays as pytest — IR internals, not user-facing) |

### Example: `tests/mdtest/type_inference/basic_types.md`

This replaces most of `tests/sql_analyzer/test_analyzer.py` (307 lines) with a readable specification:

```markdown
# Type Inference: Basic Types

## Integer types

​```sql
SELECT id::INTEGER FROM users
​```

​```python
# inferred: tuple[int]
​```

## Multiple integer subtypes

All integer-like SQL types map to Python `int`:

​```sql
SELECT a::INTEGER, b::BIGINT, c::SMALLINT, d::TINYINT FROM t
​```

​```python
# inferred: tuple[int, int, int, int]
​```

## Text types

​```sql
SELECT name::TEXT, email::VARCHAR, code::CHAR FROM users
​```

​```python
# inferred: tuple[str, str, str]
​```

## Float types

​```sql
SELECT price::FLOAT, score::DOUBLE, rating::DECIMAL FROM products
​```

​```python
# inferred: tuple[float, float, float]
​```

## Boolean type

​```sql
SELECT active::BOOLEAN FROM users
​```

​```python
# inferred: tuple[bool]
​```

## Mixed types in one query

​```sql
SELECT
    id::INTEGER,
    name::TEXT,
    price::FLOAT,
    active::BOOLEAN,
    COUNT(*)::INTEGER
FROM products
​```

​```python
# inferred: tuple[int, str, float, bool, int]
​```

## No explicit casts defaults to str

​```sql
SELECT id, name FROM users
​```

​```python
# inferred: tuple[str, str]
​```

## Single column returns single-element tuple

​```sql
SELECT COUNT(*) FROM users
​```

​```python
# inferred: tuple[int]
​```

## Invalid SQL returns None

​```sql
INVALID SQL QUERY
​```

​```python
# inferred: None
​```

## Non-SELECT query returns None

​```sql
INSERT INTO users VALUES (1, 'test')
​```

​```python
# inferred: None
​```

## RETURNING clause

​```sql
UPDATE users SET name = 'test' RETURNING id::INTEGER, name::TEXT
​```

​```python
# inferred: tuple[int, str]
​```

## PostgreSQL dialect with named parameters

​```toml
[sqlty]
dialect = "postgres"
​```

​```sql
SELECT id::INTEGER FROM users WHERE id = :id
​```

​```python
# inferred: tuple[int]
​```

## MySQL dialect with CAST

​```toml
[sqlty]
dialect = "mysql"
​```

​```sql
SELECT CAST(id AS SIGNED) FROM users
​```

​```python
# inferred: tuple[int]
​```
```

### Example: `tests/mdtest/rewrite/basic_rewrite.md`

This replaces `tests/test_magic.py` and parts of `tests/e2e/test_magic_codec_e2e.py`:

```markdown
# Rewrite: Basic Transformations

## Single integer query

​```python
q = "--sql SELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
​```

## Multiple columns

​```python
q = "--sql SELECT id::INTEGER, name::TEXT"
# rewritten: q = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT")
​```

## Triple-quoted string

​```python
q = """--sql
SELECT id::INTEGER, name::TEXT
FROM users
"""
# rewritten: q = SQL[tuple[int, str]]("""--sql\nSELECT id::INTEGER, name::TEXT\nFROM users\n""")
​```

## Non-SQL string is not rewritten

​```python
q = "Hello, World!"
# rewritten: q = "Hello, World!"
​```

## String without --sql prefix is not rewritten

​```python
q = "SELECT 1::INTEGER"
# rewritten: q = "SELECT 1::INTEGER"
​```

## Whitespace before --sql prefix

​```python
q = """  --sql
SELECT 1::INTEGER
  """
# rewritten: q = SQL[tuple[int]]("""  --sql\nSELECT 1::INTEGER\n  """)
​```

## Idempotent rewrite

Rewriting already-rewritten source produces no changes:

​```python
q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
# rewritten: q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
​```
```

### Example: `tests/mdtest/strip/round_trip.md`

This is the key new test category that doesn't exist yet:

```markdown
# Strip: Round-Trip Invariants

## Simple round-trip

​```python
q = "--sql SELECT id::INTEGER FROM users"
# roundtrip: strip(rewrite(source)) == source
​```

## Multiple queries round-trip

​```python
q1 = "--sql SELECT id::INTEGER FROM users"
q2 = "--sql SELECT name::TEXT FROM users"
# roundtrip: strip(rewrite(source)) == source
​```

## Datetime imports round-trip

​```python
q = "--sql SELECT created_at::TIMESTAMP, user_id::UUID"
# roundtrip: strip(rewrite(source)) == source
​```

## Strip removes SQL import

​```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
# stripped: q = "--sql SELECT 1::INTEGER"
# imports: -SQL
​```

## Strip removes datetime import

​```python
import datetime
from sqlty import SQL
q = SQL[tuple[datetime.datetime]]("--sql SELECT created_at::TIMESTAMP")
# stripped: q = "--sql SELECT created_at::TIMESTAMP"
# imports: -datetime, -SQL
​```

## Strip is idempotent

​```python
q = "--sql SELECT 1::INTEGER"
# stripped: q = "--sql SELECT 1::INTEGER"
​```
```

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
- **30+ scattered test files** — no unified specification of behavior

### Key insight
All operations (generate, rewrite, strip, check) answer variants of the same question: "Given a SQL string, what is its typed representation?" The pipeline is always:
1. **Find** SQL strings in source (`_collector.py` for AST, `_decode.py` for tokens — should be one)
2. **Infer** types via `SQLAnalyzer` (already unified)
3. **Format** the type expression (hardcoded as `tuple[...]` — needs a codec abstraction)
4. **Emit** into target (stub file, rewritten source, or stripped source — needs the subcommand split)

---

## Proposed Architecture

### Layer 1: Core (Rust-candidate)

Pure functions with no I/O, no Python imports, no side effects. The hotpath and natural Rust boundary:

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

**Current files:** `_sql_analyzer.py`, `_type_codec.py` (new)

**Rust migration:** `sqlparser-rs` replaces sqlglot, `TypeCodec` becomes a Rust trait, exposed via PyO3.

### Layer 2: Rewriter (Rust-candidate)

AST-level source transformation — finds SQL strings, calls Layer 1, produces transformed source:

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

**Rust migration:** `ruff_python_parser` + `ruff_python_ast` for Python AST — biggest perf win.

### Layer 3: IR + Stub Generation (Python)

Existing IR system — stays in Python, I/O-heavy, not the bottleneck.

### Layer 4: CLI (Python, thin)

ruff-style subcommand router: `generate`, `rewrite`, `strip`, `check`, `clean`, `registries`.

---

## Implementation Plan

### Phase 0: Markdown test framework

Build the mdtest runner first. All subsequent phases write mdtests before writing code.

**New files:**

| File | Purpose |
|------|---------|
| `tests/mdtest_runner.py` | Markdown parser + assertion engine (~300 lines) |
| `tests/conftest.py` (modify) | Add `pytest_collect_file` hook for `.md` discovery |
| `tests/mdtest/README.md` | Framework specification |
| `tests/mdtest/type_inference/basic_types.md` | Replace `test_analyzer.py` |
| `tests/mdtest/type_inference/datetime_types.md` | Replace datetime tests from `test_analyzer.py` |
| `tests/mdtest/type_inference/uuid_types.md` | Replace uuid tests from `test_analyzer.py` |
| `tests/mdtest/type_inference/aggregate_functions.md` | Replace `test_functions.py` |
| `tests/mdtest/type_inference/schema_inference.md` | Replace `test_sum_schema.py` |
| `tests/mdtest/type_inference/dialects.md` | Replace `test_dialects_and_schema.py` |

**Assertion types in Phase 0:**
- `# inferred: <type>` — calls `SQLAnalyzer.generate_return_type()` on preceding `sql` block
- `# inferred: None` — asserts `generate_return_type()` returns `None`
- TOML config blocks for dialect/schema

**Validation:** Run both old pytest files and new mdtests, assert identical behavior.

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
- Replaces hardcoded tuple formatting with `codec.format_type(types)`

**mdtests:** `tests/mdtest/type_codec/tuple_codec.md`, `tests/mdtest/type_codec/custom_codec.md`

### Phase 2: Unified rewriter with strip support

**Create `src/sqlty/_rewriter.py`** (~150 lines)

**Simplify `src/sqlty/_decode.py`** — thin wrapper over `SQLRewriter.rewrite()`

**mdtests:**
- `tests/mdtest/rewrite/basic_rewrite.md`
- `tests/mdtest/rewrite/multiline_queries.md`
- `tests/mdtest/rewrite/import_injection.md`
- `tests/mdtest/rewrite/idempotency.md`
- `tests/mdtest/rewrite/edge_cases.md`
- `tests/mdtest/strip/basic_strip.md`
- `tests/mdtest/strip/import_removal.md`
- `tests/mdtest/strip/idempotency.md`
- `tests/mdtest/strip/round_trip.md`

New assertion types:
- `# rewritten: <source>` — calls `SQLRewriter.rewrite()`
- `# stripped: <source>` — calls `SQLRewriter.strip()`
- `# roundtrip: strip(rewrite(source)) == source`
- `# imports: <added>` / `# imports: -<removed>`

### Phase 3: ruff-style CLI restructure

**Rewrite `src/sqlty/_cli.py`** as subcommand router.

**mdtests:**
- `tests/mdtest/generate/basic_stubs.md` (replaces `test_basic_workflow.py`)
- `tests/mdtest/generate/incremental_merge.md` (replaces `test_cli_modes.py`)
- `tests/mdtest/generate/prune.md`
- `tests/mdtest/generate/multi_registry.md` (replaces `test_multi_registry.py`)
- `tests/mdtest/generate/function_config.md` (replaces `test_function_config.py`)
- `tests/mdtest/codec/basic_codec.md` (replaces `test_magic_codec_e2e.py`)

New assertion type:
- `# stub:` with `.pyi` code block — writes files to temp dir, runs `sqlty generate`, compares output

### Phase 4: Protocol support (design only)

**Create `docs/type-codecs.md`** — architecture document for future protocol support.

### Phase 5: Rust core (optional, high-impact)

**Goal:** Replace Python hotpath with Rust via PyO3 + maturin.

The key benefit of mdtests: the **same `.md` files** test both implementations. The Rust test runner reads the same markdown, calls Rust functions instead of Python ones, and checks the same assertions.

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

**Rust crate dependencies:**

```toml
[dependencies]
pyo3 = { version = "0.22", features = ["extension-module"] }
sqlparser = "0.51"
ruff_python_parser = { git = "https://github.com/astral-sh/ruff" }
ruff_python_ast = { git = "https://github.com/astral-sh/ruff" }
```

**Fallback strategy:**

```python
try:
    from sqlty._core import rewrite, strip  # Rust (fast)
except ImportError:
    from sqlty._rewriter_py import rewrite, strip  # Pure Python fallback
```

---

## File Change Summary

### Phase 0 (mdtest framework)

| File | Purpose |
|------|---------|
| `tests/mdtest_runner.py` | Markdown parser + assertion engine |
| `tests/conftest.py` | Add `.md` collection hook |
| `tests/mdtest/README.md` | Framework specification |
| `tests/mdtest/type_inference/basic_types.md` | ~40 test cases |
| `tests/mdtest/type_inference/datetime_types.md` | ~15 test cases |
| `tests/mdtest/type_inference/uuid_types.md` | ~5 test cases |
| `tests/mdtest/type_inference/aggregate_functions.md` | ~10 test cases |
| `tests/mdtest/type_inference/schema_inference.md` | ~10 test cases |
| `tests/mdtest/type_inference/dialects.md` | ~5 test cases |

### Phase 1-3 (Python changes)

| File | Phase | Purpose |
|------|-------|---------|
| `src/sqlty/_type_codec.py` | 1 | TypeCodec Protocol + TupleCodec |
| `src/sqlty/_rewriter.py` | 2 | AST-based rewrite + strip engine |
| `src/sqlty/_sql_analyzer.py` | 1 | Accept TypeCodec parameter |
| `src/sqlty/_decode.py` | 2 | Simplify to thin wrapper |
| `src/sqlty/_cli.py` | 3 | ruff-style subcommand router |
| `tests/mdtest/type_codec/*.md` | 1 | TypeCodec tests |
| `tests/mdtest/rewrite/*.md` | 2 | Rewrite tests |
| `tests/mdtest/strip/*.md` | 2 | Strip tests |
| `tests/mdtest/generate/*.md` | 3 | Stub generation tests |
| `tests/mdtest/codec/*.md` | 3 | Magic codec tests |

### Phase 5 (Rust core)

| File | Purpose |
|------|---------|
| `Cargo.toml` | Rust workspace config |
| `src/lib.rs` | PyO3 module entry |
| `src/analyzer.rs` | SQL type inference |
| `src/rewriter.rs` | AST rewrite + strip |
| `python/sqlty/_core.pyi` | Type stubs for Rust extension |

---

## Key Design Decisions

1. **mdtest-first development** — write the markdown test before writing the code. Tests double as specification and documentation.
2. **ruff-style subcommands** — `generate`, `rewrite`, `strip`, `check`, `clean`, `registries`.
3. **`strip` as first-class command** — `sqlty strip .` undoes all `SQL[T]` wrappers in one command.
4. **TypeCodec is a Protocol** — duck typing, extensible, no inheritance.
5. **Same `.md` files test Python and Rust** — the markdown tests are the contract both implementations must satisfy.
6. **Rust core is optional** — pure Python fallback always works.
7. **Gradual migration** — old pytest files coexist with mdtests during transition, no big-bang rewrite.
8. **Backward compat** — bare `sqlty <path>` still works as `sqlty generate <path>`.
