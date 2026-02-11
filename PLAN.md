# Plan: Simplify Snapshot Codec — ruff-style CLI + Markdown Tests

> **Deferred items** (stub generation, protocol support, Rust core) are in `PLAN-FUTURE.md`.

## Problem Statement

The magic codec (`_decode.py` + `magic.py`) performs token-level transformation at Python import time, intercepting UTF-8 decoding to rewrite `"--sql ..."` strings into `SQL[tuple[...]]()` calls. This is fragile, hard to test, and has no reverse operation.

Key issues:
- `_decode.py` operates at the **token level** while the rest of the codebase uses **AST** — fundamental mismatch
- `_decode.py:_infer_sql_type()` duplicates `SQLAnalyzer.generate_return_type()`
- **No strip/reverse operation** — once rewritten, you can't undo
- **Flat CLI** — everything crammed into one argparse with flags instead of subcommands
- **No extension point** for custom type codecs (protocols, TypedDict, etc.)
- **30+ scattered test files** — no unified specification of behavior

The goal: restructure sqlty as a **ruff/uv-style multi-command tool** with a **ty-style markdown test suite**, proper CI with wheel building, and a clear separation between the core type inference engine and the CLI surface.

---

## Proposed CLI Structure (ruff/uv model)

```
sqlty <COMMAND> [OPTIONS] [PATH...]

Commands:
  rewrite     Rewrite source files in-place: "--sql ..." → SQL[T]("--sql ...")
  strip       Strip all SQL[T](...) wrappers back to plain strings
  check       Lint: validate type annotations, check rewrites are up-to-date
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
| `sqlty rewrite .` | Transform `"--sql ..."` → `SQL[T](...)` in source files | `ruff format` |
| `sqlty rewrite --check .` | Exit 1 if source files need rewriting | `ruff format --check` |
| `sqlty rewrite --diff .` | Show diff without applying | `ruff format --diff` |
| `sqlty strip .` | Reverse of rewrite: `SQL[T]("--sql ...")` → `"--sql ..."` | (no ruff analogy — unique to sqlty) |
| `sqlty strip --check .` | Exit 1 if files contain SQL[T] wrappers | — |
| `sqlty check .` | Run all lint rules (type match, star expansion, rewrite freshness) | `ruff check` |
| `sqlty check --fix .` | Auto-fix what's fixable (expand stars, update type annotations) | `ruff check --fix` |

> **Note:** `generate` (stub generation), `clean`, and `registries` commands are deferred — see `PLAN-FUTURE.md`.

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

Every assertion is a **snapshot**: the expected value lives in the `.md` file as a comment. The runner computes the actual value, compares strings, and either fails with a diff (normal mode) or rewrites the `.md` file (update mode via `pytest --update-mdtest`). This follows the same pattern as `inline-snapshot` (already a dev dependency) and ty's `snapshot-diagnostics`.

| Prefix | Compute function | Phase |
|--------|-----------------|-------|
| `# inferred:` | `analyzer.generate_return_type(sql)` | 0 |
| `# rewritten:` | `rewriter.rewrite(source)` | 2 |
| `# stripped:` | `rewriter.strip(source)` | 2 |
| `# expanded:` | `analyzer.expand_star(sql, schema)` | 2 |
| `# error:` | `checker.check(source)` — ty-style: `[rule-name] "message"` | 3 |

All assertion types share the same compare-or-update pipeline:

```python
def check_assertion(assertion, actual: str, *, update: bool, md_path: Path):
    if actual == assertion.expected:
        return  # pass
    if update:
        rewrite_line_in_md(md_path, assertion.line, actual)
    else:
        raise MDTestFailure(expected=assertion.expected, actual=actual)
```

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

**`# error: [rule-name] "message"`** — asserts a diagnostic (ty-style syntax):

```markdown
## TypedDict field count mismatch

​```python
q = SQL[User]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "User has 3 fields but query returns 2 columns"
​```
```

**`# expanded: <sql>`** — asserts star expansion output:

```markdown
## Star expands to schema columns

​```sql
SELECT * FROM users
​```

​```python
# expanded: SELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users
# inferred: tuple[int, str, str]
​```
```

**Removed assertion types** (not needed as separate types):
- ~~`# roundtrip:`~~ — test by chaining `# rewritten:` then `# stripped:` in the same test
- ~~`# imports:`~~ — visible in the full `# rewritten:` output (imports are part of the rewritten source)
- ~~`# check: ok`~~ — absence of `# error:` means no diagnostic (same as ty)

#### Update mode (`--update-mdtest`)

Like `inline-snapshot --inline-snapshot=update`, running with `--update-mdtest` rewrites assertion comments in `.md` files with actual computed values:

```bash
# Normal CI run — fails on drift
pytest tests/mdtest/ -v

# After changing the analyzer — update all expected values, review the diff
pytest tests/mdtest/ --update-mdtest
git diff tests/mdtest/  # review what changed

# Write new test — write the SQL, leave expected empty, run update
pytest tests/mdtest/postgres/new_test.md --update-mdtest
```

This is the same drift-detection pattern sqlty itself uses: `sqlty check` detects that `SQL[tuple[int, str]]` has drifted from the schema. The mdtest runner detects that `# inferred: tuple[int, str]` has drifted from the analyzer.

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
## Rewrite with schema-based inference

`schema.sql`:
​```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
​```

`pyproject.toml`:
​```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

`app.py`:
​```python
q1 = "--sql SELECT id, name FROM users"
# rewritten: q1 = SQL[tuple[int, str]]("--sql SELECT id, name FROM users")

q2 = "--sql SELECT email FROM users"
# rewritten: q2 = SQL[tuple[str]]("--sql SELECT email FROM users")
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
# rewritten: original = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
​```

​```python
from sqlty import SQL
original = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# stripped: original = "--sql SELECT id::INTEGER, name::TEXT FROM users"
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
├── postgres/                           # Comprehensive PostgreSQL documentation queries
│   ├── select.md                       # SELECT: all type casts, joins, subqueries, window fns
│   ├── returning.md                    # INSERT/UPDATE/DELETE/MERGE RETURNING
│   ├── cte.md                          # WITH / WITH RECURSIVE
│   ├── json_typing.md                  # JSON/JSONB typing with json_build_object
│   ├── type_coverage.md                # 100% SQL→Python type mapping matrix
│   ├── aggregates.md                   # Aggregate functions with schema-aware SUM
│   ├── advanced.md                     # LATERAL, window functions, set operations
│   └── star_expansion.md              # SELECT */RETURNING * with schema expansion
├── check/
│   ├── type_annotation.md             # SQL[User](...) validation against project types
│   ├── star_expansion.md              # SELECT * expansion with schema
│   └── lint_rules.md                  # Rule configuration (kebab-case rules, ty-style severity)
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
├── codec/
│   ├── basic_codec.md                  # # coding: sqlty transformation
│   ├── auto_imports.md                 # Import injection via codec
│   └── runtime_execution.md            # Transformed code runs correctly
└── type_codec/
    ├── tuple_codec.md                  # Default tuple[T1, T2] formatting
    └── custom_codec.md                 # Pluggable codec interface
```

### Test runner implementation

The runner is a **snapshot system**: parse `.md` → compute actual → compare-or-update. Same pattern as `inline-snapshot` (already a dev dep) but for markdown files.

**`tests/conftest.py`** — pytest plugin: `.md` discovery + `--update-mdtest` flag:

```python
# tests/conftest.py
import pytest
from tests.mdtest_runner import collect_mdtest_file

def pytest_addoption(parser):
    parser.addoption("--update-mdtest", action="store_true", default=False,
                     help="Update assertion comments in .md files with actual values")

def pytest_collect_file(parent, file_path):
    if file_path.suffix == ".md" and "mdtest" in str(file_path):
        return MDTestFile.from_parent(parent, path=file_path)

class MDTestFile(pytest.File):
    def collect(self):
        update = self.config.getoption("--update-mdtest", default=False)
        yield from collect_mdtest_file(self.path, update=update)

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

**`tests/mdtest_runner.py`** — markdown parser + snapshot engine (~150 lines):

The core is simple because all assertion types share the same pipeline:

```python
@dataclass
class Assertion:
    """A single assertion comment from a .md file."""
    kind: str          # "inferred", "rewritten", "stripped", "expanded", "error"
    expected: str      # The expected value after the prefix
    line: int          # Line number in the .md file (for update mode)

@dataclass
class MDTestCase:
    """A single test case extracted from a markdown heading."""
    name: str
    md_path: Path
    config: dict               # From ```toml blocks (inherited from parent sections)
    sql_blocks: list[str]      # SQL code blocks
    python_blocks: list[str]   # Python code blocks
    files: dict[str, str]      # Named file path → content
    assertions: list[Assertion]
    update: bool               # --update-mdtest mode

    def run(self):
        for assertion in self.assertions:
            actual = self._compute(assertion)
            if actual == assertion.expected:
                continue
            if self.update:
                rewrite_line_in_md(self.md_path, assertion.line, assertion.kind, actual)
            else:
                raise MDTestFailure(
                    test=self.name, kind=assertion.kind,
                    expected=assertion.expected, actual=actual,
                )

    def _compute(self, assertion: Assertion) -> str:
        match assertion.kind:
            case "inferred":
                return self._infer_type()
            case "rewritten":
                return self._rewrite()
            case "stripped":
                return self._strip()
            case "expanded":
                return self._expand_star()
            case "error":
                return self._check_diagnostics()
            case _:
                raise ValueError(f"Unknown assertion kind: {assertion.kind}")
```

**Key design properties:**
- **Snapshot-based**: same compare-or-update pattern as `inline-snapshot`
- **`--update-mdtest`**: rewrites `.md` files with actual values (write tests by writing SQL, run update)
- **Zero boilerplate**: no fixtures, no tmp_path, no subprocess calls
- **Config inheritance**: TOML blocks propagate to all assertions in nested sections
- **Multi-file tests**: named code blocks create an in-memory filesystem per test case
- **Same `.md` files** work for both Python and future Rust test runners

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
| `tests/sql_analyzer/test_analyzer.py` | `tests/mdtest/type_inference/basic_types.md` + `postgres/type_coverage.md` |
| `tests/sql_analyzer/test_functions.py` | `tests/mdtest/postgres/aggregates.md` |
| `tests/sql_analyzer/test_returning_clause.py` | `tests/mdtest/postgres/returning.md` |
| `tests/sql_analyzer/test_sum_schema.py` | `tests/mdtest/postgres/aggregates.md` (schema sections) |
| `tests/sql_analyzer/test_queries_comprehensive.py` | `tests/mdtest/postgres/select.md` + `cte.md` + `json_typing.md` + `advanced.md` |
| `tests/test_magic.py` | `tests/mdtest/rewrite/basic_rewrite.md` |
| `tests/test_magic_integration.py` | `tests/mdtest/codec/basic_codec.md` |
| `tests/e2e/test_magic_codec_e2e.py` | `tests/mdtest/codec/basic_codec.md` |
| `tests/e2e/test_dialects_and_schema.py` | `tests/mdtest/type_inference/dialects.md` |
| `tests/e2e/test_basic_workflow.py` | (stays as pytest — depends on stub generation) |
| `tests/e2e/test_multi_registry.py` | (stays as pytest — depends on stub generation) |
| `tests/e2e/test_cli_modes.py` | (stays as pytest — depends on stub generation) |
| `tests/e2e/test_function_config.py` | (stays as pytest — depends on stub generation) |
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

Round-trip tests use `# rewritten:` + `# stripped:` in sequence — no special `# roundtrip:` assertion needed:

```markdown
# Strip: Round-Trip Invariants

## Simple round-trip

Rewrite then strip produces the original source:

​```python
q = "--sql SELECT id::INTEGER FROM users"
# rewritten: q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
​```

​```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql SELECT id::INTEGER FROM users")
# stripped: q = "--sql SELECT id::INTEGER FROM users"
​```

## Strip removes SQL and datetime imports

​```python
import datetime
from sqlty import SQL
q = SQL[tuple[datetime.datetime]]("--sql SELECT created_at::TIMESTAMP")
# stripped: q = "--sql SELECT created_at::TIMESTAMP"
​```

## Strip is idempotent

​```python
q = "--sql SELECT 1::INTEGER"
# stripped: q = "--sql SELECT 1::INTEGER"
​```
```

---

## Comprehensive PostgreSQL Test Suite

The `tests/mdtest/postgres/` directory is the authoritative specification for PostgreSQL query type inference. It covers every SQL type, every DML statement with RETURNING, CTEs (including recursive), and JSON typing for known structures. Together these files exercise **100% of the SQL→Python type mapping**.

### `tests/mdtest/postgres/type_coverage.md` — Full type mapping matrix

Every SQL type in `SQL_TO_PYTHON_TYPE` must have at least one test case here. This is the single document that proves 100% coverage.

```markdown
# PostgreSQL Type Coverage

All 27 SQL types mapped to Python types. Every type must appear at least once.

## Integer family

​```sql
SELECT
    a::INT,
    b::INTEGER,
    c::BIGINT,
    d::SMALLINT,
    e::TINYINT
FROM t
​```

​```python
# inferred: tuple[int, int, int, int, int]
​```

## Float family

​```sql
SELECT
    a::FLOAT,
    b::DOUBLE,
    c::REAL,
    d::DECIMAL,
    e::NUMERIC
FROM t
​```

​```python
# inferred: tuple[float, float, float, float, float]
​```

## String family

​```sql
SELECT
    a::TEXT,
    b::VARCHAR,
    c::CHAR,
    d::STRING
FROM t
​```

​```python
# inferred: tuple[str, str, str, str]
​```

## Parameterized string types

VARCHAR(N) and CHAR(N) strip the parameter:

​```sql
SELECT name::VARCHAR(255), code::CHAR(10) FROM t
​```

​```python
# inferred: tuple[str, str]
​```

## Boolean family

​```sql
SELECT a::BOOLEAN, b::BOOL FROM t
​```

​```python
# inferred: tuple[bool, bool]
​```

## Date and time family

​```sql
SELECT
    a::DATE,
    b::TIMESTAMP,
    c::TIMESTAMPTZ,
    d::DATETIME,
    e::TIME,
    f::TIMETZ,
    g::INTERVAL
FROM t
​```

​```python
# inferred: tuple[datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta]
​```

## UUID type

​```sql
SELECT id::UUID FROM t
​```

​```python
# inferred: tuple[uuid.UUID]
​```

## Collection types

​```sql
SELECT tags::ARRAY, metadata::JSON, extra::JSONB FROM t
​```

​```python
# inferred: tuple[list, dict, dict]
​```

## All 27 types in one query

Proves every mapped type is exercised:

​```sql
SELECT
    c01::INT, c02::INTEGER, c03::BIGINT, c04::SMALLINT, c05::TINYINT,
    c06::FLOAT, c07::DOUBLE, c08::REAL, c09::DECIMAL, c10::NUMERIC,
    c11::TEXT, c12::VARCHAR, c13::CHAR, c14::STRING,
    c15::BOOLEAN, c16::BOOL,
    c17::DATE, c18::TIMESTAMP, c19::TIMESTAMPTZ, c20::DATETIME,
    c21::TIME, c22::TIMETZ, c23::INTERVAL,
    c24::ARRAY, c25::JSON, c26::JSONB,
    c27::UUID
FROM t
​```

​```python
# inferred: tuple[int, int, int, int, int, float, float, float, float, float, str, str, str, str, bool, bool, datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.time, datetime.time, datetime.timedelta, list, dict, dict, uuid.UUID]
​```

## Unknown type defaults to str

​```sql
SELECT value::CUSTOM_TYPE FROM t
​```

​```python
# inferred: tuple[str]
​```
```

### `tests/mdtest/postgres/select.md` — SELECT queries

```markdown
# PostgreSQL SELECT Queries

​```toml
[sqlty]
dialect = "postgres"
​```

## Simple SELECT with type casts

​```sql
SELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users
​```

​```python
# inferred: tuple[int, str, str]
​```

## SELECT without casts defaults to str

​```sql
SELECT id, name, email FROM users
​```

​```python
# inferred: tuple[str, str, str]
​```

## SELECT with WHERE clause

​```sql
SELECT id::INTEGER, name::TEXT FROM users WHERE active = true
​```

​```python
# inferred: tuple[int, str]
​```

## SELECT with INNER JOIN

​```sql
SELECT u.id::INTEGER, u.name::TEXT, o.total::FLOAT
FROM users u
INNER JOIN orders o ON u.id = o.user_id
​```

​```python
# inferred: tuple[int, str, float]
​```

## SELECT with LEFT JOIN and COALESCE

​```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    COALESCE(COUNT(o.id), 0)::INTEGER
FROM users u
LEFT JOIN orders o ON u.id = o.user_id
GROUP BY u.id, u.name
​```

​```python
# inferred: tuple[int, str, int]
​```

## SELECT with multiple JOINs

​```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    o.total::FLOAT,
    p.name::TEXT
FROM users u
JOIN orders o ON u.id = o.user_id
JOIN products p ON o.product_id = p.id
​```

​```python
# inferred: tuple[int, str, float, str]
​```

## Subquery in SELECT

​```sql
SELECT
    id::INTEGER,
    name::TEXT,
    (SELECT COUNT(*) FROM orders WHERE user_id = users.id)::INTEGER
FROM users
​```

​```python
# inferred: tuple[int, str, int]
​```

## SELECT DISTINCT

​```sql
SELECT DISTINCT category::TEXT, COUNT(*)::INTEGER
FROM products
GROUP BY category
​```

​```python
# inferred: tuple[str, int]
​```

## SELECT with LIMIT and OFFSET

​```sql
SELECT id::INTEGER, name::TEXT FROM users LIMIT 10 OFFSET 20
​```

​```python
# inferred: tuple[int, str]
​```

## SELECT with ORDER BY

​```sql
SELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP
FROM users
ORDER BY created_at DESC
​```

​```python
# inferred: tuple[int, str, datetime.datetime]
​```

## SELECT with CASE expression

​```sql
SELECT
    id::INTEGER,
    CASE WHEN active THEN 'active'::TEXT ELSE 'inactive'::TEXT END
FROM users
​```

​```python
# inferred: tuple[int, str]
​```

## SELECT with EXISTS subquery

​```sql
SELECT id::INTEGER, name::TEXT
FROM users u
WHERE EXISTS (SELECT 1 FROM orders WHERE user_id = u.id)
​```

​```python
# inferred: tuple[int, str]
​```

## SELECT with IN subquery

​```sql
SELECT id::INTEGER, name::TEXT
FROM users
WHERE id IN (SELECT user_id FROM active_sessions)
​```

​```python
# inferred: tuple[int, str]
​```

## Named parameters (PostgreSQL :param style)

​```sql
SELECT id::INTEGER, name::TEXT FROM users WHERE id = :user_id AND active = :active
​```

​```python
# inferred: tuple[int, str]
​```
```

### `tests/mdtest/postgres/returning.md` — INSERT/UPDATE/DELETE/MERGE RETURNING

```markdown
# PostgreSQL RETURNING Clause

​```toml
[sqlty]
dialect = "postgres"
​```

## INSERT RETURNING single column

​```sql
INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')
RETURNING id::INTEGER
​```

​```python
# inferred: tuple[int]
​```

## INSERT RETURNING multiple columns

​```sql
INSERT INTO users (name, email)
VALUES ('Alice', 'alice@example.com')
RETURNING id::INTEGER, name::TEXT, created_at::TIMESTAMP
​```

​```python
# inferred: tuple[int, str, datetime.datetime]
​```

## INSERT RETURNING all mapped types

​```sql
INSERT INTO events (name, data)
VALUES ('test', '{}')
RETURNING
    id::INTEGER,
    name::TEXT,
    score::FLOAT,
    active::BOOLEAN,
    created_at::DATE,
    updated_at::TIMESTAMP,
    scheduled::TIMESTAMPTZ,
    event_time::TIME,
    duration::INTERVAL,
    external_id::UUID,
    metadata::JSONB
​```

​```python
# inferred: tuple[int, str, float, bool, datetime.date, datetime.datetime, datetime.datetime, datetime.time, datetime.timedelta, uuid.UUID, dict]
​```

## INSERT with multiple VALUES rows

​```sql
INSERT INTO users (name, email)
VALUES ('Alice', 'alice@example.com'), ('Bob', 'bob@example.com')
RETURNING id::INTEGER, name::TEXT
​```

​```python
# inferred: tuple[int, str]
​```

## INSERT...SELECT with RETURNING

​```sql
INSERT INTO archive (user_id, name)
SELECT id, name FROM users WHERE active = false
RETURNING id::INTEGER, user_id::INTEGER, name::TEXT
​```

​```python
# inferred: tuple[int, int, str]
​```

## INSERT...ON CONFLICT DO UPDATE RETURNING

​```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) DO UPDATE SET name = EXCLUDED.name
RETURNING id::INTEGER, name::TEXT, email::TEXT
​```

​```python
# inferred: tuple[int, str, str]
​```

## INSERT...ON CONFLICT DO NOTHING RETURNING

​```sql
INSERT INTO users (email, name)
VALUES ('alice@example.com', 'Alice')
ON CONFLICT (email) DO NOTHING
RETURNING id::INTEGER
​```

​```python
# inferred: tuple[int]
​```

## UPDATE RETURNING single column

​```sql
UPDATE users SET name = 'Bob' WHERE id = 1
RETURNING id::INTEGER
​```

​```python
# inferred: tuple[int]
​```

## UPDATE RETURNING multiple columns

​```sql
UPDATE users SET name = 'Bob', email = 'bob@new.com' WHERE id = 1
RETURNING id::INTEGER, name::TEXT, email::TEXT, updated_at::TIMESTAMP
​```

​```python
# inferred: tuple[int, str, str, datetime.datetime]
​```

## UPDATE with FROM clause RETURNING

​```sql
UPDATE orders o
SET status = 'shipped'
FROM shipments s
WHERE o.id = s.order_id
RETURNING o.id::INTEGER, o.status::TEXT, s.tracking_number::TEXT
​```

​```python
# inferred: tuple[int, str, str]
​```

## UPDATE with expression in RETURNING

​```sql
UPDATE products SET price = price * 1.1
RETURNING id::INTEGER, (price * 1.1)::FLOAT
​```

​```python
# inferred: tuple[int, float]
​```

## DELETE RETURNING single column

​```sql
DELETE FROM users WHERE active = false
RETURNING id::INTEGER
​```

​```python
# inferred: tuple[int]
​```

## DELETE RETURNING multiple columns

​```sql
DELETE FROM users WHERE last_login < NOW() - INTERVAL '1 year'
RETURNING id::INTEGER, name::TEXT, email::TEXT, last_login::TIMESTAMP
​```

​```python
# inferred: tuple[int, str, str, datetime.datetime]
​```

## DELETE with USING clause RETURNING

​```sql
DELETE FROM order_items oi
USING orders o
WHERE oi.order_id = o.id AND o.status = 'cancelled'
RETURNING oi.id::INTEGER, oi.product_id::INTEGER, oi.quantity::INTEGER
​```

​```python
# inferred: tuple[int, int, int]
​```

## MERGE with RETURNING

​```sql
MERGE INTO inventory t
USING new_stock s ON t.product_id = s.product_id
WHEN MATCHED THEN UPDATE SET quantity = t.quantity + s.quantity
WHEN NOT MATCHED THEN INSERT (product_id, quantity) VALUES (s.product_id, s.quantity)
RETURNING t.product_id::INTEGER, t.quantity::INTEGER
​```

​```python
# inferred: tuple[int, int]
​```

## DML without RETURNING returns None

​```sql
INSERT INTO users (name) VALUES ('test')
​```

​```python
# inferred: None
​```

## UPDATE without RETURNING returns None

​```sql
UPDATE users SET name = 'test' WHERE id = 1
​```

​```python
# inferred: None
​```

## DELETE without RETURNING returns None

​```sql
DELETE FROM users WHERE id = 1
​```

​```python
# inferred: None
​```
```

### `tests/mdtest/postgres/cte.md` — WITH / WITH RECURSIVE

```markdown
# PostgreSQL Common Table Expressions (CTEs)

​```toml
[sqlty]
dialect = "postgres"
​```

## Simple CTE

​```sql
WITH active_users AS (
    SELECT id, name FROM users WHERE active = true
)
SELECT id::INTEGER, name::TEXT FROM active_users
​```

​```python
# inferred: tuple[int, str]
​```

## Multiple CTEs

​```sql
WITH
    active_users AS (
        SELECT id, name FROM users WHERE active = true
    ),
    user_orders AS (
        SELECT user_id, COUNT(*) as order_count
        FROM orders
        GROUP BY user_id
    )
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    COALESCE(o.order_count, 0)::INTEGER
FROM active_users u
LEFT JOIN user_orders o ON u.id = o.user_id
​```

​```python
# inferred: tuple[int, str, int]
​```

## CTE with aggregation

​```sql
WITH monthly_sales AS (
    SELECT
        product_id,
        DATE_TRUNC('month', created_at) as month,
        SUM(amount)::FLOAT as total
    FROM sales
    GROUP BY product_id, month
)
SELECT
    p.name::TEXT,
    ms.month::TIMESTAMP,
    ms.total::FLOAT
FROM monthly_sales ms
JOIN products p ON ms.product_id = p.id
​```

​```python
# inferred: tuple[str, datetime.datetime, float]
​```

## WITH RECURSIVE — tree traversal

​```sql
WITH RECURSIVE category_tree AS (
    -- Base case: root categories
    SELECT id, name, parent_id, 0 as depth
    FROM categories
    WHERE parent_id IS NULL

    UNION ALL

    -- Recursive case: children
    SELECT c.id, c.name, c.parent_id, ct.depth + 1
    FROM categories c
    JOIN category_tree ct ON c.parent_id = ct.id
)
SELECT id::INTEGER, name::TEXT, depth::INTEGER
FROM category_tree
ORDER BY depth, name
​```

​```python
# inferred: tuple[int, str, int]
​```

## WITH RECURSIVE — graph path finding

​```sql
WITH RECURSIVE path AS (
    SELECT
        id,
        name,
        ARRAY[id] as visited,
        name::TEXT as path_string,
        1 as hops
    FROM nodes
    WHERE id = 1

    UNION ALL

    SELECT
        e.target_id,
        n.name,
        p.visited || e.target_id,
        (p.path_string || ' -> ' || n.name)::TEXT,
        p.hops + 1
    FROM path p
    JOIN edges e ON p.id = e.source_id
    JOIN nodes n ON e.target_id = n.id
    WHERE NOT e.target_id = ANY(p.visited)
)
SELECT id::INTEGER, path_string::TEXT, hops::INTEGER
FROM path
WHERE id = 10
​```

​```python
# inferred: tuple[int, str, int]
​```

## WITH RECURSIVE — generate series equivalent

​```sql
WITH RECURSIVE counter AS (
    SELECT 1::INTEGER as n
    UNION ALL
    SELECT (n + 1)::INTEGER FROM counter WHERE n < 100
)
SELECT n::INTEGER FROM counter
​```

​```python
# inferred: tuple[int]
​```

## WITH RECURSIVE — organizational hierarchy

​```sql
WITH RECURSIVE org_chart AS (
    SELECT
        id, name, manager_id, title,
        1 as level
    FROM employees
    WHERE manager_id IS NULL

    UNION ALL

    SELECT
        e.id, e.name, e.manager_id, e.title,
        oc.level + 1
    FROM employees e
    JOIN org_chart oc ON e.manager_id = oc.id
)
SELECT
    id::INTEGER,
    name::TEXT,
    title::TEXT,
    level::INTEGER
FROM org_chart
ORDER BY level, name
​```

​```python
# inferred: tuple[int, str, str, int]
​```

## CTE with INSERT...RETURNING (writable CTE)

> **Known limitation:** The analyzer finds the CTE's inner RETURNING clause
> (untyped) before the outer SELECT (typed), so types come from the inner
> RETURNING. Current actual result is `tuple[str, str, str]`.
> This will be fixed when the analyzer learns to prefer outer SELECT casts.

​```sql
WITH new_user AS (
    INSERT INTO users (name, email)
    VALUES ('Alice', 'alice@example.com')
    RETURNING id, name, email
)
SELECT id::INTEGER, name::TEXT, email::TEXT FROM new_user
​```

​```python
# inferred: tuple[str, str, str]
​```

## CTE with DELETE...RETURNING (writable CTE)

> Same limitation as above — inner RETURNING wins over outer SELECT casts.

​```sql
WITH deleted_users AS (
    DELETE FROM users
    WHERE last_login < NOW() - INTERVAL '2 years'
    RETURNING id, name, email
)
SELECT id::INTEGER, name::TEXT, email::TEXT FROM deleted_users
​```

​```python
# inferred: tuple[str, str, str]
​```

## CTE with UPDATE...RETURNING (writable CTE)

> Same limitation as above.

​```sql
WITH updated_orders AS (
    UPDATE orders SET status = 'archived'
    WHERE created_at < NOW() - INTERVAL '5 years'
    RETURNING id, user_id, total, status
)
SELECT
    id::INTEGER,
    user_id::INTEGER,
    total::FLOAT,
    status::TEXT
FROM updated_orders
​```

​```python
# inferred: tuple[str, str, str, str]
​```

## Multiple writable CTEs

> Same writable CTE limitation. Inner RETURNING `id, name` (untyped) wins.

​```sql
WITH
    deactivated AS (
        UPDATE users SET active = false
        WHERE last_login < NOW() - INTERVAL '1 year'
        RETURNING id, name
    ),
    archived_orders AS (
        UPDATE orders SET status = 'archived'
        WHERE user_id IN (SELECT id FROM deactivated)
        RETURNING id, user_id, total
    )
SELECT
    d.id::INTEGER,
    d.name::TEXT,
    COUNT(a.id)::INTEGER,
    COALESCE(SUM(a.total), 0)::FLOAT
FROM deactivated d
LEFT JOIN archived_orders a ON d.id = a.user_id
GROUP BY d.id, d.name
​```

​```python
# inferred: tuple[str, str]
​```
```

### `tests/mdtest/postgres/json_typing.md` — JSON/JSONB with known-structure typing

```markdown
# PostgreSQL JSON Typing

​```toml
[sqlty]
dialect = "postgres"
​```

This file tests JSON/JSONB type inference. Currently JSON/JSONB maps to `dict`.
Future work: `json_build_object` with known keys could produce `TypedDict`.

## JSON column cast

​```sql
SELECT metadata::JSON FROM users
​```

​```python
# inferred: tuple[dict]
​```

## JSONB column cast

​```sql
SELECT metadata::JSONB FROM users
​```

​```python
# inferred: tuple[dict]
​```

## JSON field extraction with ->> (returns text)

​```sql
SELECT id::INTEGER, (data->>'name')::TEXT FROM users
​```

​```python
# inferred: tuple[int, str]
​```

## Nested JSON extraction

​```sql
SELECT id::INTEGER, (data->'address'->>'city')::TEXT FROM users
​```

​```python
# inferred: tuple[int, str]
​```

## JSON extraction with cast to non-text type

​```sql
SELECT id::INTEGER, (data->>'age')::INTEGER FROM users
​```

​```python
# inferred: tuple[int, int]
​```

## json_build_object — basic

​```sql
SELECT json_build_object(
    'id', id,
    'name', name
)::JSON FROM users
​```

​```python
# inferred: tuple[dict]
​```

## json_build_object — with typed fields (cast on outer)

​```sql
SELECT json_build_object(
    'id', id::INTEGER,
    'name', name::TEXT,
    'email', email::TEXT
)::JSON FROM users
​```

​```python
# inferred: tuple[dict]
​```

## jsonb_build_object — basic

​```sql
SELECT jsonb_build_object(
    'user_id', u.id,
    'username', u.name,
    'order_count', COUNT(o.id)
)::JSONB
FROM users u
LEFT JOIN orders o ON u.id = o.user_id
GROUP BY u.id, u.name
​```

​```python
# inferred: tuple[dict]
​```

## json_build_object — nested objects

​```sql
SELECT json_build_object(
    'user', json_build_object(
        'id', u.id,
        'name', u.name
    ),
    'address', json_build_object(
        'street', a.street,
        'city', a.city,
        'zip', a.zip_code
    )
)::JSON
FROM users u
JOIN addresses a ON u.id = a.user_id
​```

​```python
# inferred: tuple[dict]
​```

## jsonb_agg — aggregating rows as JSON array

​```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    jsonb_agg(
        jsonb_build_object('id', o.id, 'total', o.total)
    )::JSONB
FROM users u
LEFT JOIN orders o ON u.id = o.user_id
GROUP BY u.id, u.name
​```

​```python
# inferred: tuple[int, str, dict]
​```

## jsonb_agg with ORDER BY inside

​```sql
SELECT
    category::TEXT,
    jsonb_agg(name ORDER BY name)::JSONB
FROM products
GROUP BY category
​```

​```python
# inferred: tuple[str, dict]
​```

## COALESCE with JSON default

​```sql
SELECT
    u.id::INTEGER,
    COALESCE(jsonb_agg(p.title), '[]'::jsonb)::JSONB
FROM users u
LEFT JOIN posts p ON u.id = p.user_id
GROUP BY u.id
​```

​```python
# inferred: tuple[int, dict]
​```

## json_agg — non-binary JSON aggregation

​```sql
SELECT
    department::TEXT,
    json_agg(json_build_object(
        'name', name,
        'salary', salary
    ))::JSON
FROM employees
GROUP BY department
​```

​```python
# inferred: tuple[str, dict]
​```

## jsonb_object_agg — key-value aggregation

​```sql
SELECT jsonb_object_agg(key, value)::JSONB
FROM settings
WHERE scope = 'global'
​```

​```python
# inferred: tuple[dict]
​```

## json_build_object with subquery

​```sql
SELECT json_build_object(
    'user', json_build_object('id', u.id, 'name', u.name),
    'recent_orders', (
        SELECT json_agg(json_build_object('id', o.id, 'total', o.total))
        FROM orders o
        WHERE o.user_id = u.id
        ORDER BY o.created_at DESC
        LIMIT 5
    )
)::JSON
FROM users u
​```

​```python
# inferred: tuple[dict]
​```

## JSON with mixed non-JSON columns

​```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    u.created_at::TIMESTAMP,
    u.external_id::UUID,
    json_build_object(
        'preferences', u.preferences,
        'last_ip', u.last_ip
    )::JSON,
    u.active::BOOLEAN
FROM users u
​```

​```python
# inferred: tuple[int, str, datetime.datetime, uuid.UUID, dict, bool]
​```

## to_json / to_jsonb cast functions

​```sql
SELECT
    to_json(ROW(id, name))::JSON,
    to_jsonb(ROW(id, name, email))::JSONB
FROM users
​```

​```python
# inferred: tuple[dict, dict]
​```

## JSON path extraction (PG12+)

​```sql
SELECT
    id::INTEGER,
    (jsonb_path_query_first(metadata, '$.theme'))::TEXT
FROM user_settings
​```

​```python
# inferred: tuple[int, str]
​```

### Future: Typed JSON with json_build_object

> **Design note:** When `json_build_object` has literal string keys and typed value
> expressions, sqlty could infer a `TypedDict` instead of plain `dict`. This requires:
>
> 1. Recognizing `json_build_object('key1', expr1::T1, 'key2', expr2::T2, ...)`
> 2. Extracting key names from string literals
> 3. Mapping value types through the standard SQL→Python mapper
> 4. Generating `TypedDict('Result', key1=T1, key2=T2)` or `dict[str, T1 | T2]`
>
> Example (future behavior):
> ```sql
> SELECT json_build_object('id', id::INTEGER, 'name', name::TEXT)
> ```
> ```python
> # inferred: tuple[UserJson]
> # where UserJson = TypedDict('UserJson', id=int, name=str)
> ```
>
> This is Phase 4+ work and depends on the TypeCodec abstraction supporting
> complex return types. For now, all JSON builder functions return `dict`.
```

### `tests/mdtest/postgres/aggregates.md` — Aggregate functions with schema

```markdown
# PostgreSQL Aggregate Functions

​```toml
[sqlty]
dialect = "postgres"
​```

## COUNT(*) returns int

​```sql
SELECT COUNT(*) FROM users
​```

​```python
# inferred: tuple[int]
​```

## COUNT(column) returns int

​```sql
SELECT COUNT(DISTINCT email)::INTEGER FROM users
​```

​```python
# inferred: tuple[int]
​```

## SUM without cast or schema defaults to float

​```sql
SELECT SUM(amount) FROM orders
​```

​```python
# inferred: tuple[float]
​```

## AVG returns float

​```sql
SELECT AVG(score) FROM tests
​```

​```python
# inferred: tuple[float]
​```

## SUM with explicit cast

​```sql
SELECT SUM(quantity)::INTEGER FROM order_items
​```

​```python
# inferred: tuple[int]
​```

## SUM with schema-based integer inference

​```sql
SELECT SUM(quantity) FROM products
​```

`schema.sql`:
​```sql
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    quantity INTEGER NOT NULL,
    price FLOAT NOT NULL
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```python
# inferred: tuple[int]
​```

## SUM with schema-based float inference

​```sql
SELECT SUM(price) FROM products
​```

`schema.sql`:
​```sql
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    quantity INTEGER NOT NULL,
    price FLOAT NOT NULL
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```python
# inferred: tuple[float]
​```

## Explicit cast overrides schema

​```sql
SELECT SUM(quantity)::FLOAT FROM products
​```

`schema.sql`:
​```sql
CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    quantity INTEGER NOT NULL
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```python
# inferred: tuple[float]
​```

## Mixed aggregates

​```sql
SELECT
    COUNT(*)::INTEGER,
    SUM(total)::FLOAT,
    AVG(total)::FLOAT,
    MIN(created_at)::TIMESTAMP,
    MAX(created_at)::TIMESTAMP
FROM orders
​```

​```python
# inferred: tuple[int, float, float, datetime.datetime, datetime.datetime]
​```

## String aggregation

​```sql
SELECT STRING_AGG(name, ', ')::TEXT FROM users WHERE active = true
​```

​```python
# inferred: tuple[str]
​```

## GROUP BY with multiple aggregates

​```sql
SELECT
    department::TEXT,
    COUNT(*)::INTEGER,
    AVG(salary)::FLOAT,
    MIN(hire_date)::DATE,
    MAX(salary)::FLOAT
FROM employees
GROUP BY department
HAVING COUNT(*) > 5
​```

​```python
# inferred: tuple[str, int, float, datetime.date, float]
​```

## Aggregate with FILTER clause

​```sql
SELECT
    COUNT(*) FILTER (WHERE active)::INTEGER,
    COUNT(*) FILTER (WHERE NOT active)::INTEGER,
    SUM(total) FILTER (WHERE status = 'completed')::FLOAT
FROM orders
​```

​```python
# inferred: tuple[int, int, float]
​```
```

### `tests/mdtest/postgres/advanced.md` — LATERAL, window functions, set ops

```markdown
# PostgreSQL Advanced Queries

​```toml
[sqlty]
dialect = "postgres"
​```

## Window function — ROW_NUMBER

​```sql
SELECT
    id::INTEGER,
    name::TEXT,
    ROW_NUMBER() OVER (ORDER BY created_at)::INTEGER
FROM users
​```

​```python
# inferred: tuple[int, str, int]
​```

## Window function — RANK and DENSE_RANK

​```sql
SELECT
    id::INTEGER,
    name::TEXT,
    salary::FLOAT,
    RANK() OVER (PARTITION BY department ORDER BY salary DESC)::INTEGER,
    DENSE_RANK() OVER (PARTITION BY department ORDER BY salary DESC)::INTEGER
FROM employees
​```

​```python
# inferred: tuple[int, str, float, int, int]
​```

## Window function — running total

​```sql
SELECT
    id::INTEGER,
    amount::FLOAT,
    SUM(amount) OVER (ORDER BY created_at)::FLOAT
FROM transactions
​```

​```python
# inferred: tuple[int, float, float]
​```

## LATERAL join

​```sql
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    recent.total::FLOAT,
    recent.created_at::TIMESTAMP
FROM users u
CROSS JOIN LATERAL (
    SELECT total, created_at
    FROM orders
    WHERE user_id = u.id
    ORDER BY created_at DESC
    LIMIT 1
) recent
​```

​```python
# inferred: tuple[int, str, float, datetime.datetime]
​```

## UNION

​```sql
SELECT id::INTEGER, name::TEXT, 'user'::TEXT as type FROM users
UNION
SELECT id::INTEGER, name::TEXT, 'admin'::TEXT as type FROM admins
​```

​```python
# inferred: tuple[int, str, str]
​```

## UNION ALL

​```sql
SELECT id::INTEGER, email::TEXT FROM users
UNION ALL
SELECT id::INTEGER, email::TEXT FROM archived_users
​```

​```python
# inferred: tuple[int, str]
​```

## INTERSECT

​```sql
SELECT user_id::INTEGER FROM orders WHERE total > 100
INTERSECT
SELECT user_id::INTEGER FROM premium_users
​```

​```python
# inferred: tuple[int]
​```

## EXCEPT

​```sql
SELECT id::INTEGER FROM users
EXCEPT
SELECT user_id::INTEGER FROM banned_users
​```

​```python
# inferred: tuple[int]
​```

## String functions

​```sql
SELECT
    UPPER(name)::TEXT,
    LOWER(email)::TEXT,
    CONCAT(first_name, ' ', last_name)::TEXT,
    TRIM(username)::TEXT,
    LENGTH(bio)::INTEGER
FROM users
​```

​```python
# inferred: tuple[str, str, str, str, int]
​```

## Date/time functions

​```sql
SELECT
    NOW()::TIMESTAMP,
    CURRENT_DATE::DATE,
    CURRENT_TIMESTAMP::TIMESTAMP,
    created_at::TIMESTAMP,
    (created_at + INTERVAL '30 days')::TIMESTAMP,
    AGE(created_at)::INTERVAL
FROM users
​```

​```python
# inferred: tuple[datetime.datetime, datetime.date, datetime.datetime, datetime.datetime, datetime.datetime, datetime.timedelta]
​```

## Complex real-world query — user dashboard

​```sql
WITH user_stats AS (
    SELECT
        user_id,
        COUNT(*)::INTEGER as order_count,
        SUM(total)::FLOAT as total_spent,
        MAX(created_at)::TIMESTAMP as last_order
    FROM orders
    GROUP BY user_id
)
SELECT
    u.id::INTEGER,
    u.name::TEXT,
    u.email::TEXT,
    u.created_at::TIMESTAMP,
    COALESCE(s.order_count, 0)::INTEGER,
    COALESCE(s.total_spent, 0.0)::FLOAT,
    s.last_order::TIMESTAMP,
    u.active::BOOLEAN
FROM users u
LEFT JOIN user_stats s ON u.id = s.user_id
ORDER BY s.total_spent DESC NULLS LAST
​```

​```python
# inferred: tuple[int, str, str, datetime.datetime, int, float, datetime.datetime, bool]
​```

## Complex real-world query — reporting with JSON

​```sql
WITH
    daily_totals AS (
        SELECT
            DATE_TRUNC('day', created_at)::DATE as day,
            COUNT(*)::INTEGER as num_orders,
            SUM(total)::FLOAT as revenue
        FROM orders
        WHERE created_at >= CURRENT_DATE - INTERVAL '30 days'
        GROUP BY 1
    ),
    top_products AS (
        SELECT
            p.id,
            p.name,
            SUM(oi.quantity)::INTEGER as units_sold
        FROM order_items oi
        JOIN products p ON oi.product_id = p.id
        GROUP BY p.id, p.name
        ORDER BY units_sold DESC
        LIMIT 10
    )
SELECT
    dt.day::DATE,
    dt.num_orders::INTEGER,
    dt.revenue::FLOAT,
    jsonb_agg(
        jsonb_build_object('name', tp.name, 'units', tp.units_sold)
    )::JSONB
FROM daily_totals dt
CROSS JOIN top_products tp
GROUP BY dt.day, dt.num_orders, dt.revenue
ORDER BY dt.day
​```

​```python
# inferred: tuple[datetime.date, int, float, dict]
​```
```

---

## User-Annotated Type Checking

When a user manually annotates an SQL string with a project-defined type, sqlty should validate that the annotation matches the query's actual column structure.

### The problem

```python
from myapp.models import User

# User manually writes this — but does User match the query?
q = SQL[User]("--sql SELECT id, name, email FROM users")
```

If `User` is a `TypedDict`, `NamedTuple`, dataclass, or Protocol defined in the project, sqlty should verify that the SQL columns match the type's fields — both in count and in types (when schema is available).

### How it works

1. **Parse** the SQL query to extract column names and types (via `SQLAnalyzer`)
2. **Resolve** the annotated type `User` from the project AST (find its definition)
3. **Extract** the type's fields (name → Python type mapping)
4. **Compare** SQL columns against type fields
5. **Report** mismatches as lint errors

### Supported annotation types

| Python Type | How fields are extracted |
|-------------|------------------------|
| `TypedDict` | Class body with annotated fields |
| `NamedTuple` | Class body or `NamedTuple('Name', [...])` |
| `dataclass` | `@dataclass` class with annotated fields |
| `Protocol` | Class with annotated attributes/properties |
| `tuple[T1, T2, ...]` | Positional types (current default) |

### Examples

```python
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
    email: str

# VALID: query columns match User fields (count + types with schema)
q1 = SQL[User]("--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")

# error: [column-count-mismatch] — 3 fields in User, 2 columns in query
q2 = SQL[User]("--sql SELECT id::INTEGER, name::TEXT FROM users")

# error: [column-type-mismatch] — User.id is int, but FLOAT inferred
q3 = SQL[User]("--sql SELECT id::FLOAT, name::TEXT, email::TEXT FROM users")

# error: [column-name-mismatch] — User has 'email', query has 'mail'
q4 = SQL[User]("--sql SELECT id::INTEGER, name::TEXT, mail::TEXT FROM users")
```

### Type resolution strategy

The checker resolves types **statically** from the project AST — no runtime imports:

1. Parse the annotation from the `SQL[...]` subscript
2. Trace the import to find the source module
3. Parse the source module's AST to extract the type definition
4. Match field names and types against the SQL column list

This works without running the code, similar to how mypy resolves types.

### Interaction with rewrite

When the user writes a plain `--sql` string, `sqlty rewrite` infers `tuple[T1, T2, ...]` by default. But if the user has already annotated `SQL[User](...)`, `sqlty check` validates the annotation instead of replacing it.

`sqlty rewrite` **never overwrites** a user-provided type annotation. It only adds `SQL[tuple[...]]` to unannotated strings.

---

## Star Expansion and Lint Rules

### SELECT * expansion

`SELECT *` is a code smell in typed SQL — it hides which columns are returned and makes type inference impossible without a schema. With a schema available, sqlty can expand `*` to the explicit column list.

### How it works

```python
# Before (with schema defining users: id INTEGER, name TEXT, email VARCHAR)
q = "--sql SELECT * FROM users"

# After `sqlty rewrite .` (or `sqlty check --fix .`)
q = SQL[tuple[int, str, str]]("--sql SELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users")
```

The expansion:
1. Finds `SELECT *` in the SQL AST (via sqlglot `exp.Star`)
2. Looks up the table in the loaded schema
3. Replaces `*` with the explicit column list, each cast to its schema type
4. The rewriter then wraps with the correct `SQL[tuple[...]]`

For multi-table queries with `*`:
```sql
-- SELECT * with JOIN expands to all columns from both tables
SELECT * FROM users u JOIN orders o ON u.id = o.user_id
-- Expands to:
SELECT u.id::INTEGER, u.name::TEXT, u.email::VARCHAR,
       o.id::INTEGER, o.user_id::INTEGER, o.total::FLOAT
FROM users u JOIN orders o ON u.id = o.user_id
```

Table-qualified stars expand to that table's columns only:
```sql
SELECT u.*, o.total::FLOAT FROM users u JOIN orders o ON u.id = o.user_id
-- Expands to:
SELECT u.id::INTEGER, u.name::TEXT, u.email::VARCHAR, o.total::FLOAT
FROM users u JOIN orders o ON u.id = o.user_id
```

### Lint rules

Configurable rules following **ty's per-rule severity model** (not ruff's `select`/`ignore` — that pattern exists for 60+ plugin prefixes, overkill for sqlty's ~10 rules). Each rule has a kebab-case name and a severity: `error`, `warn`, or `ignore`.

```toml
[tool.sqlty]
dialect = "postgres"

[tool.sqlty.rules]
# defaults shown — override per-rule
column-count-mismatch = "error"     # Annotated type has different field count than query
column-type-mismatch = "error"      # Annotated type field has incompatible type
column-name-mismatch = "warn"       # Annotated type field name doesn't match column alias
select-star = "warn"                # SELECT * used — expand to explicit column list
select-star-no-schema = "error"     # SELECT * used but no schema available
stale-rewrite = "warn"              # Source has --sql strings not wrapped in SQL[T]
stale-type = "warn"                 # SQL[tuple[...]] type doesn't match current schema
```

| Rule | Default | Fixable | Description |
|------|---------|---------|-------------|
| `column-count-mismatch` | error | no | Annotated type has different number of fields than query columns |
| `column-type-mismatch` | error | no | Annotated type field has incompatible type with query column |
| `column-name-mismatch` | warn | no | Annotated type field name doesn't match query column alias |
| `select-star` | warn | yes | `SELECT *` used — expand to explicit column list |
| `select-star-no-schema` | error | no | `SELECT *` used but no schema available to expand |
| `stale-rewrite` | warn | yes | Source has `--sql` strings that aren't wrapped in `SQL[T]` |
| `stale-type` | warn | yes | `SQL[tuple[...]]` type doesn't match current schema (drift) |

### Configuration examples

```toml
# Strict mode: no stars allowed, all type checks enforced (defaults)
[tool.sqlty.rules]

# Lenient: allow stars, only check explicit annotations
[tool.sqlty.rules]
select-star = "ignore"

# Per-file overrides (like ty)
[[tool.sqlty.overrides]]
include = ["tests/**"]
[tool.sqlty.overrides.rules]
select-star = "ignore"
select-star-no-schema = "ignore"
```

### mdtest files for these features

```
tests/mdtest/
├── check/
│   ├── type_annotation.md            # SQL[User](...) validation
│   ├── star_expansion.md             # SELECT * expansion with schema
│   └── lint_rules.md                 # Rule configuration and error messages
├── postgres/
│   └── star_expansion.md             # PG-specific star expansion cases
```

### `tests/mdtest/check/type_annotation.md`

```markdown
# Type Annotation Checking

​```toml
[sqlty]
dialect = "postgres"
​```

## TypedDict matches query — valid

​```python
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql SELECT id::INTEGER, name::TEXT FROM users")
​```

## TypedDict field count mismatch

​```python
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
    email: str

q = SQL[User]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "User has 3 fields but query returns 2 columns"
​```

## TypedDict field type mismatch

​```python
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str

q = SQL[User]("--sql SELECT id::FLOAT, name::TEXT FROM users")
# error: [column-type-mismatch] "User.id is int but query column 'id' infers float"
​```

## TypedDict field name mismatch

​```python
from typing import TypedDict

class User(TypedDict):
    id: int
    username: str

q = SQL[User]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# error: [column-name-mismatch] "User.username doesn't match query column 'name'"
​```

## NamedTuple matches query

​```python
from typing import NamedTuple

class UserRow(NamedTuple):
    id: int
    name: str
    email: str

q = SQL[UserRow]("--sql SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
​```

## dataclass matches query

​```python
from dataclasses import dataclass

@dataclass
class UserRow:
    id: int
    name: str
    created_at: datetime.datetime

q = SQL[UserRow]("--sql SELECT id::INTEGER, name::TEXT, created_at::TIMESTAMP FROM users")
​```

## tuple annotation is not name-checked

​```python
q = SQL[tuple[int, str]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
​```

## tuple annotation count mismatch

​```python
q = SQL[tuple[int, str, float]]("--sql SELECT id::INTEGER, name::TEXT FROM users")
# error: [column-count-mismatch] "tuple has 3 elements but query returns 2 columns"
​```

## Annotation imported from another module

`models.py`:
​```python
from typing import TypedDict

class Product(TypedDict):
    id: int
    name: str
    price: float
​```

`app.py`:
​```python
from models import Product

q = SQL[Product]("--sql SELECT id::INTEGER, name::TEXT, price::FLOAT FROM products")
​```
```

### `tests/mdtest/check/star_expansion.md`

```markdown
# SELECT * Expansion

## Star expands to schema columns

`schema.sql`:
​```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```sql
SELECT * FROM users
​```

​```python
# inferred: tuple[int, str, str]
# expanded: SELECT id::INTEGER, name::TEXT, email::VARCHAR FROM users
​```

## Star with no schema is an error

​```sql
SELECT * FROM users
​```

​```python
# error: [select-star-no-schema] "SELECT * used but no schema available"
​```

## Table-qualified star

`schema.sql`:
​```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    total FLOAT NOT NULL
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```sql
SELECT u.*, o.total::FLOAT FROM users u JOIN orders o ON u.id = o.user_id
​```

​```python
# expanded: SELECT u.id::INTEGER, u.name::TEXT, o.total::FLOAT FROM users u JOIN orders o ON u.id = o.user_id
# inferred: tuple[int, str, float]
​```

## Star in RETURNING clause

`schema.sql`:
​```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255)
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

​```sql
INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com') RETURNING *
​```

​```python
# expanded: INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com') RETURNING id::INTEGER, name::TEXT, email::VARCHAR
# inferred: tuple[int, str, str]
​```

## Star expansion with lint rule disabled

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"

[sqlty.rules]
select-star = "ignore"
​```

​```sql
SELECT * FROM users
​```

​```python
# inferred: tuple[int, str, str]
​```
```

### `tests/mdtest/postgres/star_expansion.md`

```markdown
# PostgreSQL Star Expansion

​```toml
[sqlty]
dialect = "postgres"
​```

`schema.sql`:
​```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW(),
    active BOOLEAN DEFAULT true
);

CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    total FLOAT NOT NULL,
    status TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    price FLOAT NOT NULL,
    category TEXT,
    metadata JSONB
);
​```

​```toml
[sqlty]
dialect = "postgres"
schema = "schema.sql"
​```

## Simple star on single table

​```sql
SELECT * FROM users
​```

​```python
# expanded: SELECT id::INTEGER, name::TEXT, email::VARCHAR, created_at::TIMESTAMP, active::BOOLEAN FROM users
# inferred: tuple[int, str, str, datetime.datetime, bool]
​```

## Star with WHERE clause

​```sql
SELECT * FROM users WHERE active = true
​```

​```python
# expanded: SELECT id::INTEGER, name::TEXT, email::VARCHAR, created_at::TIMESTAMP, active::BOOLEAN FROM users WHERE active = true
# inferred: tuple[int, str, str, datetime.datetime, bool]
​```

## Star with JOIN

​```sql
SELECT * FROM users u JOIN orders o ON u.id = o.user_id
​```

​```python
# expanded: SELECT u.id::INTEGER, u.name::TEXT, u.email::VARCHAR, u.created_at::TIMESTAMP, u.active::BOOLEAN, o.id::INTEGER, o.user_id::INTEGER, o.total::FLOAT, o.status::TEXT, o.created_at::TIMESTAMP FROM users u JOIN orders o ON u.id = o.user_id
# inferred: tuple[int, str, str, datetime.datetime, bool, int, int, float, str, datetime.datetime]
​```

## Table-qualified star with explicit columns

​```sql
SELECT u.*, o.total::FLOAT, o.status::TEXT
FROM users u
JOIN orders o ON u.id = o.user_id
​```

​```python
# expanded: SELECT u.id::INTEGER, u.name::TEXT, u.email::VARCHAR, u.created_at::TIMESTAMP, u.active::BOOLEAN, o.total::FLOAT, o.status::TEXT FROM users u JOIN orders o ON u.id = o.user_id
# inferred: tuple[int, str, str, datetime.datetime, bool, float, str]
​```

## Star in CTE

​```sql
WITH active_users AS (
    SELECT * FROM users WHERE active = true
)
SELECT id::INTEGER, name::TEXT FROM active_users
​```

​```python
# inferred: tuple[int, str]
​```

## RETURNING * after INSERT

​```sql
INSERT INTO products (name, price, category)
VALUES ('Widget', 9.99, 'tools')
RETURNING *
​```

​```python
# expanded: INSERT INTO products (name, price, category) VALUES ('Widget', 9.99, 'tools') RETURNING id::INTEGER, name::TEXT, price::FLOAT, category::TEXT, metadata::JSONB
# inferred: tuple[int, str, float, str, dict]
​```

## RETURNING * after UPDATE

​```sql
UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year'
RETURNING *
​```

​```python
# expanded: UPDATE users SET active = false WHERE last_login < NOW() - INTERVAL '1 year' RETURNING id::INTEGER, name::TEXT, email::VARCHAR, created_at::TIMESTAMP, active::BOOLEAN
# inferred: tuple[int, str, str, datetime.datetime, bool]
​```

## RETURNING * after DELETE

​```sql
DELETE FROM orders WHERE status = 'cancelled'
RETURNING *
​```

​```python
# expanded: DELETE FROM orders WHERE status = 'cancelled' RETURNING id::INTEGER, user_id::INTEGER, total::FLOAT, status::TEXT, created_at::TIMESTAMP
# inferred: tuple[int, int, float, str, datetime.datetime]
​```

## Mixed star and explicit columns

​```sql
SELECT *, COUNT(*) OVER () as total_count::INTEGER FROM products WHERE category = 'tools'
​```

​```python
# expanded: SELECT id::INTEGER, name::TEXT, price::FLOAT, category::TEXT, metadata::JSONB, COUNT(*) OVER () as total_count::INTEGER FROM products WHERE category = 'tools'
# inferred: tuple[int, str, float, str, dict, int]
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
- `_transform_sql_string()` hardcodes `SQL[tuple[int]]("--sql ...")` at line 110 — no extension point
- **No strip/reverse operation** — once you rewrite, you can't undo
- **Flat CLI** — everything crammed into one argparse with flags (`--check`, `--mode`) instead of subcommands
- **30+ scattered test files** — no unified specification of behavior

### Key insight
All operations (rewrite, strip, check) answer variants of the same question: "Given a SQL string, what is its typed representation?" The pipeline is always:
1. **Find** SQL strings in source (`_decode.py` for tokens — should use AST)
2. **Infer** types via `SQLAnalyzer` (already unified)
3. **Format** the type expression (hardcoded as `tuple[...]` — needs a codec abstraction)
4. **Emit** into target (rewritten source or stripped source — needs the subcommand split)

---

## Proposed Architecture

### Layer 1: Core

Pure functions with no I/O, no Python imports, no side effects:

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

### Layer 2: Rewriter

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

### Layer 3: CLI (Python, thin)

ruff-style subcommand router: `rewrite`, `strip`, `check`.

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
| `tests/mdtest/postgres/type_coverage.md` | 100% SQL→Python type mapping (all 27 types) |
| `tests/mdtest/postgres/select.md` | SELECT: joins, subqueries, window functions, CASE |
| `tests/mdtest/postgres/returning.md` | INSERT/UPDATE/DELETE/MERGE RETURNING (~20 cases) |
| `tests/mdtest/postgres/cte.md` | WITH / WITH RECURSIVE (~12 cases incl. writable CTEs) |
| `tests/mdtest/postgres/json_typing.md` | JSON/JSONB: json_build_object, jsonb_agg, extraction (~18 cases) |
| `tests/mdtest/postgres/aggregates.md` | COUNT, SUM, AVG with schema-aware inference |
| `tests/mdtest/postgres/advanced.md` | LATERAL, window functions, UNION/INTERSECT/EXCEPT |

**Assertion types in Phase 0:**
- `# inferred: <type>` — calls `SQLAnalyzer.generate_return_type()` on preceding `sql` block
- `# inferred: None` — asserts `generate_return_type()` returns `None`
- TOML config blocks for dialect/schema

**Validation:** Run both old pytest files and new mdtests, assert identical behavior.

### Phase 1: TypeCodec abstraction ✅ DONE

**Created `src/sqlty/_type_codec.py`** (~40 lines):
- `TypeCodec` Protocol with `format_type(column_types: list[str]) -> str`
- `TupleCodec` default implementation: `tuple[int, str]`
- Module-level `_default_codec` instance to avoid re-creation per call

**Modified `src/sqlty/_sql_analyzer.py`:**
- `generate_return_type()` accepts optional `codec: TypeCodec | None` parameter
- Defaults to `TupleCodec()` — fully backward-compatible (all 204 existing tests pass unchanged)
- Single integration point: all callers (`_decode.py`, `_generator.py`, mdtest runner) get codec support for free

**mdtests:** `tests/mdtest/type_codec/tuple_codec.md` (4 cases), `tests/mdtest/type_codec/custom_codec.md` (4 cases)

**Key design insight:** All type formatting flows through ONE location (`generate_return_type:337-340`). The TypeCodec is a drop-in replacement — no cascading changes needed.

**Known limitations discovered:**
- Table aliases (`u.id` vs `users.id`) don't resolve against schema — schema lookup uses exact table name match
- Writable CTEs: analyzer finds inner RETURNING before outer SELECT (documented in cte.md)

### Phase 2: Unified rewriter with strip and star expansion

**Created `src/sqlty/_rewriter.py`** (~180 lines) ✅ DONE:
- `SQLRewriter.rewrite(source)` — `"--sql ..."` → `SQL[T](...)` with import injection
- `SQLRewriter.strip(source)` — reverse: `SQL[T](...)` → `"--sql ..."` with import removal
- Idempotent: already-wrapped strings are not double-wrapped
- Regex-based string detection (single, double, triple-quoted)
- Accepts optional `TypeCodec` for custom type formatting
- `expand_stars(sql, schema)` — deferred to Phase 3 (requires deeper sqlglot integration)

**`src/sqlty/_decode.py`** — NOT simplified yet (deferred). The decode module works at the codec/bytes level; the rewriter works at the source/string level. Both share `SQLAnalyzer` but have different entry points.

**mdtests implemented:**
- `tests/mdtest/rewrite/basic_rewrite.md` (5 cases: simple, multi-column, non-SQL, no-prefix, idempotent)
- `tests/mdtest/strip/basic_strip.md` (4 cases: simple, multi-column, idempotent, non-SQL)
- `tests/mdtest/strip/round_trip.md` (4 cases: rewrite→strip and strip→rewrite invariants)
- Star expansion tests (`expanded:` assertion) — deferred to Phase 3

New assertion types implemented in mdtest_runner.py:
- `# rewritten: <source>` — calls `SQLRewriter.rewrite()`, compares code (imports stripped)
- `# stripped: <source>` — calls `SQLRewriter.strip()`, compares code

Design decision: `# rewritten:` assertions show only the code transformation, not imports.
This is cleaner for single-line assertions and avoids multi-line matching complexity.

**Remaining Phase 2 items (deferred):**
- `SQLRewriter.expand_stars(sql, schema)` — `SELECT *` → explicit column list with casts
- `# expanded: <sql>` assertion type
- `# error: [rule-name]` assertion type (Phase 3)

### Phase 3: ruff-style CLI with check/lint

**Rewrite `src/sqlty/_cli.py`** as subcommand router with `rewrite`, `strip`, `check`.

**Create `src/sqlty/_checker.py`** (~200 lines):
- Type annotation resolver — AST-based, finds TypedDict/NamedTuple/dataclass definitions
- Field extractor — extracts (name, type) pairs from resolved types
- Column matcher — compares SQL columns against type fields
- Lint rule runner — kebab-case rules with ty-style per-rule severity

**Create `src/sqlty/_lint.py`** (~100 lines):
- Lint rule registry and configuration loading from `[tool.sqlty.rules]`
- Per-rule severity: `error` / `warn` / `ignore`
- `[[tool.sqlty.overrides]]` for per-file rule config

**mdtests:**
- `tests/mdtest/check/type_annotation.md` — SQL[User](...) validation
- `tests/mdtest/check/lint_rules.md` — rule configuration
- `tests/mdtest/codec/basic_codec.md` (replaces `test_magic_codec_e2e.py`)

**Note:** CI updates happen incrementally. Phase 0 adds the mdtest runner to CI. Phase 3 updates the test workflow to split unit/mdtest/e2e runs. See the **Project Setup and CI** section below for full details.

---

## File Change Summary

### Phase 0 (mdtest framework + postgres test suite) ✅ DONE

| File | Status | Purpose |
|------|--------|---------|
| `tests/mdtest_runner.py` | ✅ | Markdown parser + assertion engine (~280 lines) |
| `tests/conftest.py` | ✅ | pytest plugin for `.md` discovery + `--update-mdtest` flag |
| `tests/mdtest/type_inference/basic_types.md` | ✅ | 16 test cases |
| `tests/mdtest/type_inference/schema_inference.md` | ✅ | 8 test cases |
| `tests/mdtest/postgres/type_coverage.md` | ✅ | 10 test cases (all 27 types) |
| `tests/mdtest/postgres/select.md` | ✅ | 11 test cases |
| `tests/mdtest/postgres/returning.md` | ✅ | 13 test cases |
| `tests/mdtest/postgres/cte.md` | ✅ | 6 test cases |
| `tests/mdtest/postgres/json_typing.md` | ✅ | 10 test cases |
| `tests/mdtest/postgres/aggregates.md` | ✅ | 7 test cases |
| `tests/mdtest/postgres/advanced.md` | ✅ | 7 test cases |

### Phase 1 (TypeCodec) ✅ DONE

| File | Status | Purpose |
|------|--------|---------|
| `src/sqlty/_type_codec.py` | ✅ | TypeCodec Protocol + TupleCodec (~40 lines) |
| `src/sqlty/_sql_analyzer.py` | ✅ | Accept optional `codec` parameter |
| `tests/mdtest/type_codec/tuple_codec.md` | ✅ | 4 test cases |
| `tests/mdtest/type_codec/custom_codec.md` | ✅ | 4 test cases |

### Phase 2 (Rewriter) ✅ DONE (core; star expansion deferred)

| File | Status | Purpose |
|------|--------|---------|
| `src/sqlty/_rewriter.py` | ✅ | SQLRewriter.rewrite() + strip() (~180 lines) |
| `tests/mdtest/rewrite/basic_rewrite.md` | ✅ | 5 test cases |
| `tests/mdtest/strip/basic_strip.md` | ✅ | 4 test cases |
| `tests/mdtest/strip/round_trip.md` | ✅ | 4 test cases |

### Phase 0 (CI updates — alongside mdtest)

| File | Purpose |
|------|---------|
| `.github/workflows/tests.yml` | Add mdtest runner, expand Python version matrix |

### Phase 2-3 (remaining Python changes)

| File | Phase | Purpose |
|------|-------|---------|
| `src/sqlty/_sql_analyzer.py` | 2 | Star expansion (`expand_star()`) |
| `src/sqlty/_rewriter.py` | 2 | AST-based rewrite + strip + star expansion |
| `src/sqlty/_decode.py` | 2 | Simplify to thin wrapper |
| `src/sqlty/_checker.py` | 3 | Type annotation resolver + column matcher |
| `src/sqlty/_lint.py` | 3 | Lint rule registry (kebab-case, ty-style severity) |
| `src/sqlty/_cli.py` | 3 | ruff-style subcommand router |
| `tests/mdtest/type_codec/*.md` | 1 | TypeCodec tests |
| `tests/mdtest/rewrite/*.md` | 2 | Rewrite tests |
| `tests/mdtest/strip/*.md` | 2 | Strip tests |
| `tests/mdtest/check/*.md` | 3 | Type annotation + star expansion + lint tests |
| `tests/mdtest/postgres/star_expansion.md` | 2 | PG-specific star expansion cases |
| `tests/mdtest/codec/*.md` | 3 | Magic codec tests |

### Future (multi-platform wheel building — when Rust is added)

| File | Purpose |
|------|---------|
| `pyproject.toml` | Switch build backend to maturin |
| `.github/workflows/wheels.yml` | New: multi-platform wheel building |
| `.github/workflows/pypi.yml` | Update for wheel artifact collection |

---

## Project Setup and CI

### Build system

The current build system uses **hatchling** with `uv-dynamic-versioning`. This remains the build backend for the pure-Python package. When a Rust extension is added later (see `PLAN-FUTURE.md`), the build backend switches to **maturin**.

#### Current (pure Python — no changes needed for Phase 0-3)

```toml
[build-system]
requires = ["hatchling", "uv-dynamic-versioning"]
build-backend = "hatchling.build"
```

#### Future (with Rust extension — deferred)

```toml
[build-system]
requires = ["maturin>=1.7,<2.0"]
build-backend = "maturin"

[tool.maturin]
python-source = "src"
module-name = "sqlty._core"
features = ["pyo3/extension-module"]
```

### No crate publishing needed

Rust is used **only** as a compiled extension inside the Python wheel. There is no need to publish crates to crates.io. The Rust code lives in the same repo and is compiled during wheel building.

### CI: Updated test workflow

Update `.github/workflows/tests.yml` to run mdtests alongside existing pytest:

```yaml
name: Tests

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

jobs:
  test:
    name: Test Python ${{ matrix.python-version }}
    runs-on: ubuntu-latest
    strategy:
      matrix:
        python-version:
          - "3.12"
          - "3.13"
          - "3.14"
      fail-fast: false

    steps:
      - uses: actions/checkout@v4

      - name: Install uv
        uses: astral-sh/setup-uv@v5
        with:
          enable-cache: true
          python-version: ${{ matrix.python-version }}

      - name: Install the project
        run: uv sync --all-groups

      - name: Run unit tests
        run: uv run pytest tests/ -v --ignore=tests/e2e

      - name: Run mdtests
        run: uv run pytest tests/mdtest/ -v

      - name: Run e2e tests
        run: uv run pytest tests/e2e/ -v

  lint:
    name: Lint
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install uv
        uses: astral-sh/setup-uv@v5
        with:
          enable-cache: true

      - name: Run pre-commit
        run: uvx pre-commit run --all-files

      - name: Show git diff
        if: failure()
        run: git --no-pager diff || true
```

### CI: Multi-platform wheel building (future, when Rust is added)

When the Rust extension is introduced, add `.github/workflows/wheels.yml`:

```yaml
name: Build wheels

on:
  release:
    types: [published]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  build-wheels:
    name: Build wheel for ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          # Linux x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          # Linux aarch64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
          # macOS x86_64
          - os: macos-13
            target: x86_64-apple-darwin
          # macOS aarch64 (Apple Silicon)
          - os: macos-14
            target: aarch64-apple-darwin
          # Windows x86_64
          - os: windows-latest
            target: x86_64-pc-windows-msvc

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install uv
        uses: astral-sh/setup-uv@v5

      - name: Build wheels
        uses: PyO3/maturin-action@v1
        with:
          target: ${{ matrix.target }}
          args: --release --out dist
          manylinux: auto

      - name: Upload wheels
        uses: actions/upload-artifact@v4
        with:
          name: wheels-${{ matrix.target }}
          path: dist/*.whl

  # Also build pure-Python sdist
  build-sdist:
    name: Build sdist
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install uv
        uses: astral-sh/setup-uv@v5

      - name: Build sdist
        run: uv build --sdist

      - name: Upload sdist
        uses: actions/upload-artifact@v4
        with:
          name: sdist
          path: dist/*.tar.gz

  publish:
    name: Publish to PyPI
    if: github.event_name == 'release'
    needs: [build-wheels, build-sdist]
    runs-on: ubuntu-latest
    environment:
      name: pypi
      url: https://pypi.org/p/sqlty
    permissions:
      id-token: write

    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist/
          merge-multiple: true

      - uses: pypa/gh-action-pypi-publish@release/v1
```

**Key points:**
- **No crate publishing** — only Python wheels are published to PyPI
- **5 platform targets** — Linux (x86_64, aarch64), macOS (Intel, Apple Silicon), Windows (x86_64)
- **`maturin-action`** handles cross-compilation with `manylinux: auto`
- **Pure Python sdist** always available as fallback (installs without Rust)
- **Trusted publishing** via PyPI OIDC (no API tokens needed)

---

## Key Design Decisions

1. **mdtest-first development** — write the markdown test before writing the code. Tests double as specification and documentation.
2. **ruff-style subcommands** — `rewrite`, `strip`, `check` (stub generation, registries deferred).
3. **`strip` as first-class command** — `sqlty strip .` undoes all `SQL[T]` wrappers in one command.
4. **`check` validates user annotations** — `SQL[User](...)` is checked against the resolved type's fields. Never overwrite user-provided types.
5. **Star expansion** — `SELECT *` expands to explicit column list from schema. `select-star` lint rule to disallow `*`.
6. **Lint rules follow ty conventions** — kebab-case names (`column-count-mismatch`), per-rule severity (`error`/`warn`/`ignore`) via `[tool.sqlty.rules]`.
7. **TypeCodec is a Protocol** — duck typing, extensible, no inheritance.
8. **Comprehensive PostgreSQL test suite** — 100% type coverage, RETURNING, CTEs, JSON builder functions.
9. **Gradual migration** — old pytest files coexist with mdtests during transition, no big-bang rewrite.
10. **Wheels only, no crates** — Rust (when added) is compiled into Python wheels, not published to crates.io.
