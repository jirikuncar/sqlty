# AGENTS.md — Development conventions for AI agents and contributors

Use `uv` for all Python commands. Use editor capabilities (e.g. VSCode) to run and debug tests.

## Quick reference

```bash
uv sync --all-groups               # install
uv run pytest                      # all tests
make test-mdtest                   # mdtest only
make update-mdtest                 # update mdtest assertions
make generate-docs                 # regenerate reference tables (configuration.md, cli.md)
make lint-docs                     # rumdl (Markdown) and Vale (prose) on the docs
make typecheck                     # mypy + ty on src and examples
uv run prek run --all-files        # all hooks (run before every commit)
```

- `SQL[T]` is a `str` subclass. It works anywhere a string works.
- Every new feature gets an mdtest (see [Markdown tests](#markdown-tests-mdtest)).
- Review rules for pull requests live in [REVIEW.md](REVIEW.md).
- Plans live in `plans/`, named `YYYY-MM-DD-<slug>.md` after the day they were written.

## AI disclosure

Follow the [AI-assisted contributions](CONTRIBUTING.md#ai-assisted-contributions)
policy: end every commit message with a `Co-Authored-By:` trailer naming the
agent, and say in the pull request description which agent was used and for what.

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

## Inference rules

Operators, functions, aggregates and JSON accessors are typed by `*.inference.toml`
files in `rust/crates/sqlty-sql/rules/`, compiled by `build.rs` into static match arms.
Type a new function or operator with a rule, not in Rust: a rule carries its reference
and reasoning, and `--explain` shows it. When a result needs code (it depends on an
argument's value, say), add a `@` result kind to `compile.rs` and cite the rule.
[`rules/README.md`](rust/crates/sqlty-sql/rules/README.md) has the format, the
lookup order and how to verify a `reference`. Every rule needs a `reference` to
vendor documentation of the version the tests target (PostgreSQL 17, MySQL 8.4,
SQLite 3), checked against the published page, and a one-sentence `reasoning`;
the build fails otherwise. `sqlty check --explain` and `-- rules:` mdtest
assertions show which rule typed a column; every rule needs one, in
`tests/mdtest/type_inference/rule_coverage.md` if nowhere else
(`test_every_inference_rule_is_asserted`). `rule_dispatch_does_not_allocate`
(`analyzer/inference.rs`) keeps rule lookup free of heap allocations.

## Agent Skills

`skills/<name>/SKILL.md` (and its `references/`) are the Agent Skills, installed
through the plugin manifests (`.claude-plugin/`, `.codex-plugin/`,
`.cursor-plugin/`) or `sqlty skill --install`. `rust/crates/sqlty-ext/build.rs`
compiles them into the extension: rebuild it after editing one, or
`test_agent_skills_are_compiled_in` fails. A new skill gets a row in the table of
`docs/recipes/agents.md`.

## Benchmarks

Three layers, all in the Makefile:

- `make bench-ci` (`benchmarks/bench_project.py`, with `tests/test_performance.py`)
  is end-to-end: CLI wall time and RSS on a synthetic project, gated by
  `benchmarks/baseline.json`. Needs a release build (`maturin develop --release`). Published wheels use
  the slower `dist` profile (fat LTO, one codegen unit; `rust/Cargo.toml`), so `release` numbers can
  differ slightly from a shipped wheel.
- `make bench-rust` runs Criterion benches in `rust/crates/sqlty-bench`: catalog
  load (270 tables with functions), analyzer per query kind, and
  `check_source`/`rewrite_source` on 1k/4k queries. Use it to find where time
  goes and to compare a branch (`ARGS="--save-baseline main"`, then
  `ARGS="--baseline main"`).
- `make bench-codspeed` runs `benchmarks/test_benchmarks.py` with pytest-codspeed: each
  query kind of `synth.KINDS` through `analyze_select`/`analyze_params`, the schema
  load, `rewrite_source` and `check_source`. CI's CodSpeed job counts their
  instructions on every commit and reports the change on the pull request.

## Git safety

**Never force-push.** Do not use `git push --force`, `git push --force-with-lease`,
or any other force-push variant. If a push is rejected, investigate and resolve
the conflict without rewriting remote history. No exceptions.

## Hooks

**Always run the hooks before every commit:**

```bash
uv run prek run --all-files
```

[prek](https://prek.j178.dev) runs the hooks of `prek.toml`; `uv run prek install`
runs them on every commit. Each runs when the files it depends on change:

| Hook | Runs when these change |
|------|------------------------|
| mypy and ty on `src/` | `src/`, `pyproject.toml` |
| `sqlty rewrite` (in place), `sqlty check`, mypy and ty on `examples/` (except `magic_codec/`) | `examples/`, `src/sqlty/`, `rust/crates/` |
| `cargo fmt --check` | Rust files |
| `cargo clippy -D warnings`, `cargo test` | `rust/` |
| ruff (lint + format), uv-lock | Python files, `pyproject.toml` |
| file checks (whitespace, line endings, YAML/TOML/JSON syntax, ...) | any file |

Hooks that edit files run one at a time; the read-only checks run in
parallel. CI's Lint job runs the same hooks on all files. Fix all errors before
creating a commit.

## The `--sql\n` convention

**Always use `--sql\n` (newline escape) after the `--sql` prefix, not a space.**

`--sql SELECT ...` with a space is a SQL comment — the parser sees an
empty query and the string is **not** wrapped. The newline makes the
parser treat `--sql` as a comment line and the query starts on the next line.

```python
# Correct — parser sees --sql as comment, SELECT as the query
q = "--sql\nSELECT id::INTEGER FROM users"

# Wrong — entire line is a SQL comment, parser sees empty query
q = "--sql SELECT id::INTEGER FROM users"
```

`rewrite`/`strip` never modify string content, only the `SQL[T]()` wrapper,
so `strip(rewrite(x)) == x` for files without wrappers or a hand-written
import line that `strip` removes (`from sqlty import SQL`, `from sqlty import
UNTYPED`, or the configured `import-style` line). `check --fix` is the exception: its
`inline-sql-comment`, `select-star` and `mysql-sql-marker` fixes edit content.
`-- sql\n` is an alternative marker that MySQL also reads as a comment.
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
`sqlty rule NAME`). Every test runs through `sqlty.core`.

### Where to add tests

User-visible behavior belongs in `docs/guide/` too; edge cases only in
`tests/mdtest/`. A new lint rule needs `src/sqlty/rules/<name>.md`.

| Feature area | Directory | Example file |
|-------------|-----------|-------------|
| Type inference | `tests/mdtest/type_inference/` | `basic_types.md` |
| SQL dialects | `tests/mdtest/postgres/`, `mysql/`, `sqlite/` | `select.md` |
| Nullability | `tests/mdtest/nullability/` | `inference.md` |
| Rewriting | `tests/mdtest/rewrite/` | `basic_rewrite.md` |
| Stripping | `tests/mdtest/strip/` | `basic_strip.md` |
| Lint rules | `tests/mdtest/check/` | `type_annotation.md` |
| Magic codec | `tests/mdtest/codec/` | `basic_codec.md` |

### Structure

- A heading with assertions is a test, named by its headings joined with
  ` - ` (the H1 title excluded).
- Setup is inherited by nested headings, code is not: an unlabelled `toml`
  block is `[tool.sqlty]` config (unknown keys and rules are errors); a
  `` `schema.sql`: `` label before a block makes it a file; a labelled `.py`
  file is a helper module (`pkg/db.py` is `pkg.db`) for `# error:` checks.
- Each block with assertions is checked on its own, together with the
  assertion-free python blocks right before it (literate setup); later
  blocks are not part of it. Prose may sit between blocks.
- Other languages (`bash`, `text`, ...) and blocks tagged ```` ```python ignore ````
  are prose.
- Directives: `<!-- mdtest:skip reason -->`, `<!-- mdtest:xfail reason -->`.
- An `` `against.sql`: `` file makes `# error:` assert what `sqlty breaking`
  reports (`core.breaking_source()`): the diagnostics that are new when the code
  is checked against that schema instead of its own.

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
| `-- columns:` | in the `sql` block | `core.analyze_select()`: `name: type (SQL_TYPE)` per line |
| `-- json:` | in the `sql` block | `core.json_shapes()`: `name: shape` per JSON column of known shape |
| `-- rules:` | in the `sql` block | `core.explain_select()`: `name: type-rule-id / nullability-rule-id` per column, `-` for none |
| `-- params:` | in the `sql` block | `core.analyze_params()`: `number-or-name: type (SQL_TYPE)` per parameter, then ` NULL` or ` NOT NULL` when known |
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

`uv run pytest 'tests/mdtest/check/user_types.md::TypedDict - field type mismatch'`
reruns one test; failures print this command.

- Tests must use `--sql\n` (not `--sql ` with space) — space makes the line a SQL comment.

## Documentation philosophy

### Docstrings

Write docstrings that explain **how to use** the function, not just what it does.
Include examples that an AI agent or a developer new to the codebase can copy-paste.
Use the `Example:` doctest convention:

```python
def analyze_select(sql: str, *, dialect: str = "postgres") -> list[ColumnInfo] | None:
    """Result columns of a query, or ``None`` when it returns no rows or doesn't parse.

    Example:
        >>> analyze_select("SELECT id::INTEGER FROM users")
        [ColumnInfo(name='id', python_type='int', sql_type='INT')]
    """
```

### CLI `--help` text

CLI help text should be concise but complete enough for an AI agent to use the
tool without reading source code. Include the most common usage pattern first:

```text
sqlty rewrite .          # "--sql\n..." → SQL[T]("--sql\n...")
sqlty check .            # lint for type mismatches
sqlty strip .            # SQL[T]("--sql\n...") → "--sql\n..."
```

### README and docs

- README.md, `docs/` and `src/sqlty/rules/` are mdtest files; their assertions run with the suite.
- `tests/test_core_invariants.py` checks idempotency and round trips over every
  mdtest code block: new mdtests extend it automatically.
- `docs/reference/configuration.md` and `cli.md` hold tables between `<!-- generated:NAME -->`
  markers, rendered by `scripts/generate_docs.py` from `KEY_DOCS` in `_config.py` and the argparse
  parser. Edit those, then run `make generate-docs`; `test_generated_reference_is_current` fails on drift.
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
make typecheck-src             # mypy + ty on src/sqlty (also a prek hook)
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
| Rust functions (`sqlty._core`) | Read from `_core.pyi` | Read from `_core.pyi` | Keep the stub in sync with `rust/crates/sqlty-ext/src/lib.rs` |
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

```text
src/sqlty/
├── __init__.py          # SQL[T] generic str subclass
├── core.py              # Public API over the Rust engine
├── _check.py            # Python check passes of check_source: row types, parameters
├── _core.pyi            # Stub for sqlty._core (rust/crates/sqlty-ext/src/lib.rs)
├── _type_extractor/     # Row types (TypedDict, dataclass, ...), read from source, never imported
├── _lint.py             # Lint rules: severity, diagnostics, RuleConfig
├── _decode.py           # Magic codec decoder
├── _cli.py              # CLI: rewrite, strip, check, version
└── magic.py             # Python codec registration
rust/crates/             # Cargo workspace (root: rust/Cargo.toml)
├── sqlty-sql/           # analyzer/ (type inference), ddl; no PyO3
│   └── rules/           # inference rules (*.inference.toml), compiled by build.rs; see its README
├── sqlty-python/        # tokens, rewriter, checker, fixer
└── sqlty-ext/           # lib.rs (PyO3), builds sqlty._core
proofs/                  # Lean 4 model of SqlType::python, with proofs; see its README
```

`proofs/SqlTy/Table.lean` is generated from `scalar_python`: after changing the scalar
mapping, run `SQLTY_UPDATE_PROOFS=1 cargo test -p sqlty-sql lean_table` and `lake build`
in `proofs/`.
