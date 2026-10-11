# AGENTS.md — Development conventions for AI agents and contributors

Use `uv` for all Python commands. Use editor capabilities (e.g. VSCode) to run and debug tests.

## Running tests

```bash
uv run pytest                              # all tests (577+)
uv run pytest tests/mdtest docs src/sqlty/rules README.md  # mdtests only
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
make typecheck       # mypy + ty on src and examples
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

Hooks defined in `.pre-commit-config.yaml`:

- mypy and ty on `src/`
- `sqlty check` on `examples/`; `sqlty rewrite` in place on `examples/`
  (except `magic_codec/`)
- `cargo fmt --check` on `rust/` (requires `cargo`)
- ruff (lint + format) and uv-lock
- standard file checks (trailing whitespace, YAML/TOML/JSON syntax, etc.)

CI's Lint job additionally runs `make typecheck-examples` and `cargo clippy`,
so a clean local pre-commit does not guarantee a green Lint job. Fix all
errors before creating a commit.

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
`check --fix` may edit string content only for `inline-sql-comment` and
`select-star`; `rewrite` and `strip` still keep that guarantee.
For triple-quoted multiline strings, use a real newline:

```python
q = """--sql
SELECT id::INTEGER, name::TEXT FROM users"""
```

## Markdown tests (mdtest)

Every new feature, bug fix, or behavior change **must** have an mdtest. The
format follows [ty's mdtest](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md);
the plugin is `tests/_mdtest/`, and `mdtest_paths` in `pyproject.toml` lists
the collected files: `tests/mdtest/` (specification), `docs/` and `README.md`
(user docs, also the Zensical site), `src/sqlty/rules/` (one page per lint rule, printed by
`sqlty rule NAME`). Every test runs through `sqlty.core` on each backend:
`name[py]`, `name[rs]`.

### Where to add tests

User-visible behavior belongs in `docs/guide/` too; edge cases only in
`tests/mdtest/`. A new lint rule needs `src/sqlty/rules/<name>.md`.

| Feature area | Directory | Example file |
|-------------|-----------|-------------|
| Type inference | `tests/mdtest/type_inference/` | `basic_types.md` |
| SQL dialects | `tests/mdtest/postgres/`, `mysql/`, `sqlite/` | `select.md` |
| Rewriting | `tests/mdtest/rewrite/` | `basic_rewrite.md` |
| Stripping | `tests/mdtest/strip/` | `basic_strip.md` |
| Lint rules | `tests/mdtest/check/` | `type_annotation.md` |
| Magic codec | `tests/mdtest/codec/` | `basic_codec.md` |

### Structure

- A heading with assertions is a test, named by its headings joined with
  ` - ` (the H1 title excluded).
- Setup is inherited by nested headings, code is not: an unlabelled `toml`
  block is `[tool.sqlty]` config (unknown keys and rules are errors); a
  `` `schema.sql`: `` label before a block makes it a file.
- Each block with assertions is checked on its own, together with the
  assertion-free python blocks right before it (literate setup); later
  blocks are not part of it. Prose may sit between blocks.
- Other languages (`bash`, `text`, ...) and blocks tagged ```` ```python ignore ````
  are prose.
- Directives: `<!-- mdtest:skip reason -->`, `<!-- mdtest:xfail reason -->`,
  `<!-- mdtest:backend py -->`.

````markdown
## Schema-based TypedDict mismatch

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER, name TEXT);
```

```python
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

# error: [column-count-mismatch] "User has 1 fields but query returns 2 columns"
q = SQL[User]("--sql\nSELECT id, name FROM users")
```
````

### Assertions

| Assertion | Where | Compared with |
|-----------|-------|---------------|
| `-- inferred:` | in the `sql` block | `core.infer_type()` |
| `-- expanded:` | in the `sql` block | `core.expand_star()` |
| `-- columns:` | in the `sql` block | `SQLAnalyzer.analyze_select()`: `name: type (SQL_TYPE)` per line |
| `# rewritten:` | `python`, whole code | `core.rewrite_source()` without imports |
| `# rewritten-file:` | `python`, whole code | `core.rewrite_source()` |
| `# stripped:` | `python`, whole code | `core.strip_source()` |
| `# fixed:` | `python`, whole code | `core.fix_source()` (`sqlty check --fix`) |
| `# decoded:` | `python`, whole code | magic codec `decode()` without imports or the `# coding:` line |
| `# error: [rule] "message"` | end of its line, or on the line above | `core.check_source()` |

- Consecutive comments of one kind form a multi-line value.
- `# error:` takes a rule, a message substring, or both. A test with any
  `# error:` fails on every diagnostic it doesn't assert; a bare `# error:`
  asserts there are none.

### Workflow

1. Write the code with empty assertions (`# rewritten:`, `# error:`).
2. `make update-mdtest` fills them in; errors are written above their lines.
3. Review the diff, then commit.

`uv run pytest 'tests/mdtest/check/user_types.md::TypedDict - field type mismatch[rs]'`
reruns one test; failures print this command.

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

- README.md, `docs/` and `src/sqlty/rules/` are mdtest files; their assertions run with the suite.
- `tests/test_core_invariants.py` checks idempotency, round trip and backend agreement
  over every mdtest code block: new mdtests extend it automatically.
- `tests/test_docs.py` checks that the rules table in `docs/reference/rules.md`
  matches `RULES`, that every rule has a page in `src/sqlty/rules/`, that
  relative links resolve, that `examples/` need no rewrite, and runs the
  `>>>` examples in `docs/reference/python-api.md`.
- Focus on **motivated examples** — show the problem first, then the solution.
- Always show complete working code, not snippets.

## Type checker support

`src/sqlty/` and the examples must pass both **mypy** (strict) and **ty**
with zero errors. `SQL[T]` usage is additionally checked with mypy, pyright
and ty by `tests/test_typecheckers.py`.

```bash
make typecheck                 # mypy + ty on src and examples
make typecheck-src             # mypy + ty on src/sqlty (also a pre-commit hook)
make typecheck-examples        # mypy + ty on each examples/ dir (runs in CI)
uv run pytest tests/test_typecheckers.py -v  # mypy, pyright, ty on tests/typechecks/
```

`examples/magic_codec/` is excluded: its `# coding: sqlty` header is
decoded at import time, which type checkers cannot do.

### Checker differences

| Case | mypy | ty | Convention |
|------|------|----|------------|
| Setting attributes on a function (`fn.__sqlty__ = True`) | `attr-defined` | `invalid-assignment` / `unresolved-attribute` | Suppress both on the same line |
| `__name__` of a `Callable` | Allowed | Not part of `Callable` | `getattr(fn, "__name__", "unknown")` |
| Backend imports in `core.py` (`_core.pyi` stub and `_core_py.py`) | Checks every branch | Checks every branch | Keep signatures identical |
| Optional `psycopg` dependency | `ignore_missing_imports` override in `pyproject.toml` | Installed by `uv sync --all-groups` | No inline suppression |

### Suppression comments

- mypy: `# type: ignore[error-code]`; ty: `# ty: ignore[rule-name]`.
- Always name the code or rule. When both checkers complain about one line,
  put both comments on it: `# type: ignore[attr-defined]  # ty: ignore[invalid-assignment]`.
- mypy runs with `warn_unused_ignores`, so drop suppressions that are no longer needed.

### Adding type checker tests

Each `tests/typechecks/sql_*.py` file is run through mypy, pyright and ty,
which must all report zero errors. Use `reveal_type()` to document the
inferred types, then run `uv run pytest tests/test_typecheckers.py -v`.
Files that must produce errors (like `untyped_misuse.py`) need their own
test with an explicit expected error count.

## Architecture overview

```
src/sqlty/
├── __init__.py          # SQL[T] generic str subclass
├── core.py              # Public API: try Rust _core, fall back to _core_py
├── _core_py.py          # Pure-Python: infer_type, rewrite_source, strip_source, check_source
├── _rewriter.py         # SQLRewriter: whole-source regex rewrite/strip
├── _fixer.py           # fix_source: sqlty check --fix, built on sqlty.core
├── _sql_analyzer.py     # SQL parsing + type inference (sqlglot)
├── _lint.py             # Lint rules: severity, diagnostics, RuleConfig
├── _checker.py          # Column validation, SELECT * detection
├── _decode.py           # Magic codec decoder
├── _cli.py              # CLI: rewrite, strip, check, version
└── magic.py             # Python codec registration
```
