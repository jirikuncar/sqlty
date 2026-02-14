# AGENTS.md — Development conventions for AI agents and contributors

Use `uv` for all Python commands. Use editor capabilities (e.g. VSCode) to run and debug tests.

## Running tests

```bash
uv run pytest                              # all tests (577+)
uv run pytest tests/mdtest/ -v             # mdtest suite only
uv run pytest tests/ --ignore=tests/e2e --ignore=tests/mdtest  # unit tests only
uv run pytest tests/e2e/ -v               # end-to-end tests
uv run pytest -k "integer cast"           # filter by name
uv run pytest tests/mdtest/rewrite/basic_rewrite.md -v  # single file
```

Make targets:

```bash
make test            # all tests
make test-mdtest     # mdtest only
make test-unit       # unit tests only
make test-e2e        # e2e only
make update-mdtest   # auto-update assertion comments in .md files
make typecheck       # mypy + ty
```

## Git safety

**Never force-push.** Do not use `git push --force`, `git push --force-with-lease`,
or any other force-push variant. If a push is rejected, investigate and resolve
the conflict without rewriting remote history. No exceptions.

## Pre-commit checks

**Always run pre-commit before every commit:**

```bash
uv run pre-commit run --all-files
```

This runs mypy, ruff (lint + format), trailing whitespace fixes, and other
checks defined in `.pre-commit-config.yaml`. Fix all errors before creating
a commit.

## The `--sql\n` convention

**Always use `--sql\n` (newline escape) after the `--sql` prefix, not a space.**

`--sql SELECT ...` with a space is a SQL comment — the parser sees an
empty query and the string is **not** wrapped.  The newline makes the
parser treat `--sql` as a comment line and the query starts on the next line.

```python
# Correct — parser sees --sql as comment, SELECT as the query
q = "--sql\nSELECT id::INTEGER FROM users"

# Wrong — entire line is a SQL comment, parser sees empty query
q = "--sql SELECT id::INTEGER FROM users"
```

The rewriter never modifies string content — it only adds/removes the
`SQL[T]()` wrapper.  This guarantees `strip(rewrite(source)) == source`.
For triple-quoted multiline strings, use a real newline:

```python
q = """--sql
SELECT id::INTEGER, name::TEXT FROM users"""
```

## Adding mdtests for new features

Every new feature, bug fix, or behavior change **must** have a corresponding
mdtest in `tests/mdtest/`. The test framework is snapshot-based: assertion
comments in Python code blocks are compared against computed values.

### Where to add tests

| Feature area | Directory | Example file |
|-------------|-----------|-------------|
| Type inference | `tests/mdtest/type_inference/` | `basic_types.md` |
| SQL dialects | `tests/mdtest/postgres/`, `mysql/`, `sqlite/` | `select.md` |
| Rewriting | `tests/mdtest/rewrite/` | `basic_rewrite.md` |
| Stripping | `tests/mdtest/strip/` | `basic_strip.md` |
| Lint rules | `tests/mdtest/check/` | `type_annotation.md` |
| Magic codec | `tests/mdtest/codec/` | `basic_codec.md` |
| Type codecs | `tests/mdtest/type_codec/` | `tuple_codec.md` |

### Writing a new test

1. Add a `##` heading in the appropriate `.md` file (or create a new one):

````markdown
## New feature description

```python
q = "--sql\nSELECT id::INTEGER FROM users"
# rewritten:
```
````

2. Run `make update-mdtest` to auto-fill the expected value.

3. Verify the result makes sense, then commit.

### Assertion types

| Prefix | Tests | Computed by |
|--------|-------|------------|
| `# inferred:` | Type inference result | `SQLAnalyzer.generate_return_type()` |
| `# rewritten:` | Rewritten source (no imports) | `SQLRewriter.rewrite()` |
| `# rewritten-file:` | Rewritten source (with imports) | `SQLRewriter.rewrite()` |
| `# stripped:` | Stripped source | `SQLRewriter.strip()` |
| `# expanded:` | Star expansion | `SQLAnalyzer.expand_star()` |
| `# error:` | Lint diagnostic | `Checker.check_query()` |
| `# decoded:` | Magic codec output | `decode()` |
| `# warning:` | Rewriter warning | `SQLRewriter.rewrite().warnings` |

Multi-line assertions use consecutive same-prefix comments:

```python
q = """--sql
SELECT id::INTEGER FROM users"""
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("""--sql
# rewritten-file: SELECT id::INTEGER FROM users""")
```

### Keeping tests updated

- Run `make update-mdtest` after any change to rewriter, type inference, or lint logic.
- README.md is also tested via `tests/test_readme.py` — keep assertion comments there current.
- Tests must use `--sql\n` (not `--sql ` with space) — space makes the line a SQL comment.

## Documentation philosophy

### Docstrings

Write docstrings that explain **how to use** the function, not just what it does.
Include examples that an AI agent or a developer new to the codebase can copy-paste.
Use the `Example:` doctest convention:

```python
def rewrite(self, source: str) -> RewriteResult:
    """Transform --sql prefixed strings into SQL[T](...) calls.

    Example:
        >>> r = SQLRewriter()
        >>> r.rewrite('q = "--sql\\nSELECT id::INTEGER FROM users"').source
        'from sqlty import SQL\\nq = SQL[tuple[int]]("--sql\\\\nSELECT id::INTEGER FROM users")'
    """
```

### CLI `--help` text

CLI help text should be concise but complete enough for an AI agent to use the
tool without reading source code. Include the most common usage pattern first:

```
sqlty rewrite .          # "--sql\n..." → SQL[T]("--sql\n...")
sqlty check .            # lint for type mismatches
sqlty strip .            # SQL[T]("--sql\n...") → "--sql\n..."
```

### README and docs

- README.md code blocks with assertion comments (`# rewritten:`, `# decoded:`, etc.)
  are tested automatically. Keep them accurate.
- Focus on **motivated examples** — show the problem first, then the solution.
- Always show complete working code, not snippets.

## Type checker support

Both **mypy** and **ty** (plus pyright) are tested against `src/sqlty/` and
all examples. Run `make typecheck` to execute both.

### Running type checks

```bash
make typecheck                 # mypy + ty on src and examples
uv run mypy src/sqlty          # mypy only
uv run ty check src/sqlty      # ty only
uv run pytest tests/test_typecheckers.py -v  # all three checkers on test files
```

### Type checker–specific quirks

| Issue | mypy | ty | Fix |
|-------|------|----|-----|
| Optional deps (`psycopg`, `isort`) | `[[tool.mypy.overrides]]` with `ignore_missing_imports` | `# ty: ignore[unresolved-import]` inline | Both configured |
| `codecs.CodecInfo` `streamreader` arg | Accepts class directly | Expects `_StreamReader` callable type | `# ty: ignore[invalid-argument-type]` in `magic.py` |
| Rust stub signatures (`_core.pyi`) | Must match Python backend | Union of both backends checked strictly | Keep `_core.pyi` and `_core_py.py` signatures aligned |
| `Callable` `__name__` attribute | Allowed on `Callable` | Not part of `Callable` protocol | Use `getattr(fn, "__name__", "unknown")` |

### Suppression comments

- **mypy**: `# type: ignore[error-code]` — suppress a specific mypy error
- **ty**: `# ty: ignore[rule-name]` — suppress a specific ty diagnostic
- Never use bare `# type: ignore` without an error code

### Adding type check test files

Type checker validation files live in `tests/typechecks/sql_*.py`. Each file
is tested against mypy, pyright, and ty via `tests/test_typecheckers.py`.
To add a new test file:

1. Create `tests/typechecks/sql_<name>.py`
2. Use `reveal_type()` to verify inferred types
3. Run `uv run pytest tests/test_typecheckers.py -v` to check all three

## Architecture overview

```
src/sqlty/
├── __init__.py          # SQL[T] generic str subclass
├── core.py              # Public API: try Rust _core, fall back to _core_py
├── _core_py.py          # Pure-Python: infer_type, rewrite_source, strip_source, check_source
├── _rewriter.py         # SQLRewriter: whole-source regex rewrite/strip
├── _sql_analyzer.py     # SQL parsing + type inference (sqlglot)
├── _lint.py             # Lint rules: severity, diagnostics, RuleConfig
├── _checker.py          # Column validation, SELECT * detection
├── _type_codec.py       # TypeCodec protocol + TupleCodec
├── _decode.py           # Magic codec decoder
├── _cli.py              # CLI: rewrite, strip, check, version
└── magic.py             # Python codec registration
```
