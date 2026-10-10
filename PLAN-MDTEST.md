# Plan: Documentation as Tests (ty mdtest pattern)

Goal: one Markdown corpus that is both the user documentation and the
behavioral specification, executed by a single pytest plugin against both
backends. Custom Python tests shrink to what Markdown cannot express.

## 1. What ty does (verified against astral-sh/ruff @ a5aee68)

Sources:
[ty_test README](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md),
[mdtest parser](https://github.com/astral-sh/ruff/blob/main/crates/mdtest/src/parser.rs),
[harness](https://github.com/astral-sh/ruff/blob/main/crates/ty_python_semantic/tests/mdtest.rs),
[resources/mdtest](https://github.com/astral-sh/ruff/tree/main/crates/ty_python_semantic/resources/mdtest),
[resources/lint_docs](https://github.com/astral-sh/ruff/tree/main/crates/ty_python_semantic/resources/lint_docs).

| Pattern | ty | sqlty today |
|---|---|---|
| Layout | 362 files in topic dirs, max 2 levels deep, snake_case | 38 files, 7 dirs, matches |
| Test name | headings joined: `file.md - A - B - C` | leaf heading only, collisions possible |
| Section rules | a section has code **or** sub-headings; mixing is a parse error | silently accepted |
| Code blocks | unnamed `py` blocks concatenate (literate style) | only the last `python` block is used |
| Named files | `` `b.py`: `` label before a block | supported for `.sql` and `.py` |
| Config | one `toml` block per section, inherited, **unknown keys rejected** | inherited; parse errors swallowed (`except Exception: pass`) |
| Directives | HTML comments, unknown ones are errors (`snapshot-diagnostics`, `expect-panic`, ...) | none |
| Assertions | inline, per line: `# error: 8 [rule] "msg"`, `# revealed: T` | per block, first diagnostic only |
| Unasserted output | unexpected diagnostic fails the test | ignored |
| Rule docs | `lint_docs/<rule>.md` is `include_str!`'d as the rule doc **and** run as mdtest; bare `# error` defaults to the file's rule | rule table in `docs/api.md`, untested |
| Prose | H1 + intro paragraph, spec links, `## Basic` first, `TODO` for known gaps | similar, mostly good |
| Runner | `datatest_stable` over two roots, `MDTEST_TEST_FILTER`, prints exact rerun command | pytest collector on paths containing `mdtest` |

User-facing ty docs (astral-sh/ty `docs/`) are *not* executed; only
`lint_docs` are. sqlty can go further because its docs are mostly code.

## 2. Findings in sqlty

1. **Examples drift.** `examples/02_schema_inference/queries.py` contains
   `tuple[str, str, str]`; its README and a real
   `sqlty rewrite --schema schema.sql` produce `tuple[int, str, str]`.
   Root cause: the `sqlty-rewrite` pre-commit hook runs from the repo root
   without `--schema`, and the example has no `pyproject.toml`.
2. **Docs are untested.** `docs/*.md` and `examples/*/README.md` have 0
   assertions; only `README.md` runs via the bespoke `tests/test_readme.py`.
3. **mdtests bypass the public API and the Rust backend.** The runner calls
   `SQLAnalyzer`/`SQLRewriter`/`Checker` directly, so `test_core_conformance.py`
   (108 tests) re-specifies the same behavior for `_core`.
4. **User-type checks live only in `sqlty.core`.** `_core_py.check_source` and
   `_core.check_source` return `[]` for a mismatching `SQL[User](...)`; the
   facade adds it. `_core_py.check_source`'s docstring claims otherwise.
5. **Synthetic annotations.** `check/user_types.md` describes types via
   `[sqlty.annotation] fields = [...]` TOML instead of real Python — unreadable
   as docs and not what users write. `typeddict_qualifiers.md` already shows
   the better form.
6. **Two config dialects** (`[sqlty]` and `[tool.sqlty]`). Users can only copy
   the second.
7. **Runner hygiene.** `tempfile.mktemp` for schemas (the core API already takes
   `schema_sql`), swallowed TOML errors, no failure location in output.

## 3. Target layout

```
docs/                          # published AND executed
  index.md
  installation.md
  motivation.md
  guide/
    quick-start.md             # replaces examples/01 + README quick start
    schema-inference.md        # examples/02
    configuration.md           # examples/03
    user-types.md
    import-style.md            # examples/05
    untyped.md
    magic-codec.md
    psycopg.md
  rules/                       # one file per LintRule, ty lint_docs style
    column-count-mismatch.md   # ## What it does / ## Why is this bad? / ## Examples
    ...                        # 12 files, `# error` defaults to the file's rule
  reference/
    cli.md                     # generated from argparse, CI checks freshness
    configuration.md           # generated from _config.py keys
    type-mappings.md           # executed table of `# inferred:` cases
tests/
  mdtest/                      # specification, not published (ty resources/mdtest)
    type_inference/ postgres/ mysql/ sqlite/
    rewrite/ strip/ check/ codec/ type_codec/
    regression/                # one file per fixed issue, named gh_<n>.md
  _mdtest/                     # the plugin (replaces mdtest_runner.py + conftest bits)
    __init__.py
    parser.py                  # Markdown -> Section tree, strict validation
    model.py                   # Section, File, Assertion, Config (dataclasses)
    assertions.py              # kind -> compute function over sqlty.core
    update.py                  # MDFileUpdater
    plugin.py                  # pytest hooks, options, reporting
  unit/                        # what Markdown cannot express
    test_stub_ir.py test_type_extractor.py test_psycopg.py ...
  e2e/                         # subprocess / import-hook / real DB
  typechecks/ + test_typecheckers.py   # unchanged; reveal_type across mypy/pyright/ty
examples/                      # kept only as runnable projects, verified as fixpoints
```

`README.md` stays executable and links into `docs/guide/`.

## 4. Markdown format (sqlty mdtest v2)

Borrow ty's rules, keep sqlty's assertion kinds.

````markdown
# Schema inference

Without casts, column types come from `CREATE TABLE` statements.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

## Basic

```py
q = "--sql\nSELECT id, name FROM users"  # rewritten: q = SQL[tuple[int, str]]("--sql\nSELECT id, name FROM users")
```

## User type mismatch

```py
from typing import TypedDict
from sqlty import SQL

class User(TypedDict):
    id: str

q = SQL[User]("--sql\nSELECT id FROM users")  # error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
```
````

Rules:

- Config only as `[tool.sqlty]`; validated against `_config.py`, unknown keys fail.
- `` `name`: `` labels materialize files (`schema.sql`, `pyproject.toml`,
  `pkg/models.py`) in an in-memory map; `schema` resolves from it and is
  passed as `schema_sql`.
- Unnamed `py` blocks in a section concatenate; `sql` blocks feed `# inferred:`
  and `# expanded:`.
- Assertions attach to the line they end, or to the next code line when they
  stand alone (ty placement). Block-level kinds (`rewritten-file`, `stripped`)
  keep the current consecutive-comment form.
- `# error:` matches `[rule]`, `"substring"`, optional leading column; every
  diagnostic returned by `check_source` must be matched, so `# error:` with no
  value is no longer needed.
- Directives: `<!-- mdtest:skip reason -->`, `<!-- mdtest:xfail reason -->`,
  `<!-- mdtest:backend py -->`; unknown comments fail.
- Blocks tagged `bash`, `console`, `text`, `yaml`, or with `ignore` are prose.
- A section with code and sub-headings is an error.

## 5. pytest integration

```toml
[tool.pytest.ini_options]
testpaths = ["tests", "docs", "README.md"]
mdtest_paths = ["tests/mdtest", "docs", "README.md"]
```

- `tests/conftest.py`: `pytest_plugins = ["tests._mdtest.plugin"]`.
- `pytest_collect_file` matches `.md` under `mdtest_paths` (not the
  `"mdtest" in parts` heuristic); `tests/test_readme.py` is deleted.
- Item id: `docs/guide/user-types.md::User type mismatch - TypedDict[rs]`.
  Backend parametrization reuses `--sqlty-lib`; the plugin monkeypatches the
  `sqlty.core._backend_*` bindings per item, so every assertion runs through
  the public facade on both `py` and `rs`.
- Failure report: `path.md:LINE`, expected/actual diff, and the
  `uv run pytest 'path.md::name[rs]'` rerun line.
- `--update-mdtest` unchanged; add `--update-mdtest=new` to fill only empty
  assertions.
- `make docs-test` = `uv run pytest docs README.md`.

Example projects: one parametrized test copies each `examples/*` dir to
`tmp_path`, runs `strip` then `rewrite` with that dir's config, and asserts
the files are byte-identical. This catches finding 1 generically. Give
`examples/02_schema_inference` a `pyproject.toml` and make the pre-commit hook
loop per directory.

## 6. Migration of custom tests

| Current | Lines / tests | Action |
|---|---|---|
| `test_core_conformance.py` | 1096 / 108 | Move value assertions to `tests/mdtest/**`; keep only API-shape and invariant tests (types returned, idempotency, `strip(rewrite(x)) == x`) as a ~150-line property module |
| `sql_analyzer/test_*.py` | 1827 / 124 | Move to `tests/mdtest/type_inference` and `postgres/returning.md` (duplicates exist) |
| `test_core.py` | 267 / 22 | Merge leftovers into the invariant module |
| `test_magic.py`, `test_magic_issue.py` | 195 / 15 | `tests/mdtest/codec/*.md` with `# decoded:` |
| `test_readme.py` | 21 | Deleted, README collected by plugin |
| `test_mdtest_runner.py` | 127 | Becomes `tests/_mdtest/test_parser.py` plus `tests/mdtest/mdtest_format.md` (ty's `mdtest_config.md` equivalent) |
| `generator/`, `ir/`, `test_type_extractor.py`, `test_psycopg.py`, `e2e/`, `test_typecheckers.py` | — | Keep: subprocess, filesystem, DB, external checkers |

Expected: ~3,000 lines of Python tests removed, every behavior asserted on
both backends, docs and examples unable to drift.

## 7. Steps

1. Fix finding 1 (example 02 config, per-directory pre-commit hook) and the
   `_core_py.check_source` docstring.
2. Extract the plugin into `tests/_mdtest/`, route assertions through
   `sqlty.core`, add backend parametrization, keep current syntax passing.
3. Strict parser: full test names, section validation, TOML validation,
   directives, literate concatenation.
4. Inline per-line `# error:` / `# rewritten:` with unmatched-diagnostic
   failure; convert `[sqlty.annotation]` tests to real Python; drop `[sqlty]`.
5. Collect `docs/` and `README.md`; add assertions to docs, create
   `docs/guide/` from `examples/*/README.md`.
6. `docs/rules/*.md`; `LintRule` loads its description from the file and
   `sqlty check --explain <rule>` prints it.
7. Migrate custom tests per section 6, deleting each module once its cases
   pass as Markdown on both backends.
8. Generated `docs/reference/cli.md` with a freshness test.
