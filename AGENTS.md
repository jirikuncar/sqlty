# AGENTS.md — Development conventions for AI agents and contributors

Use `uv` for all Python commands. Setup, test and build commands and the
architecture live in [DEVELOPMENT.md](DEVELOPMENT.md).

- `SQL[T]` is a `str` subclass. It works anywhere a string works.
- Run `uv run prek run --all-files` before every commit and fix every error it
  reports; [DEVELOPMENT.md](DEVELOPMENT.md#hooks) lists the hooks.
- Never force-push, `--force-with-lease` included: resolve a rejected push without rewriting
  remote history.
- Every change gets an mdtest: see [Markdown tests](#markdown-tests-mdtest).
- Review rules for pull requests live in [REVIEW.md](REVIEW.md). Before handing a
  branch over, have a separate agent review it with the
  [`review` skill](.claude/skills/review/SKILL.md), given the branch name only.
  `.claude/skills/` is for contributors and not shipped, unlike `skills/`.
- [THREAT_MODEL.md](THREAT_MODEL.md) lists the trust boundaries a change must
  keep: no execution of analyzed code, generated code, files written.
- Plans live in `plans/`, named `YYYY-MM-DD-<slug>.md` after the day they were written.
  Delete a plan once it has shipped; move what is left to `plans/2026-02-13-future.md`
  or an issue.

## AI disclosure

Follow the [AI-assisted contributions](CONTRIBUTING.md#ai-assisted-contributions)
policy: end every commit message with a `Co-Authored-By:` trailer naming the
agent, and say in the pull request description which agent was used and for what.

## Writing tests

Tests go through the public API of the package they test: a Rust crate's from its
`tests/` against its `pub` items, sqlty's from `sqlty`, `sqlty.core`, the CLI or
`sqlty._core` (sqlty-ext's API), never a private `sqlty._*` module. Keep a test in `src/`
only for what no public call can observe, such as an allocation or a catalog-entry count.
`scripts/internal-tests.txt` counts what does not yet, per package (`#[test]`s in `src/`,
private uses in pytest files);
`tests/test_public_api_tests.py` fails when one rises and asks to lower it when one falls.

Tests run in parallel and in random order. A test must not depend on another
having run, nor on the working directory or global state another changes.
A test that measures wall time is marked `timing`.

A test that needs a package sqlty does not depend on imports it at the top, and
the package goes in a dependency group (`[dependency-groups]` in `pyproject.toml`),
which `uv sync --all-groups` and CI install. Don't use `pytest.importorskip`: a
missing package would skip the test silently, and `No-exec harness intact` fails on
any new skip. Skip only on what CI does not provide everywhere: a server
(`SQLTY_PG_DSN`), a package installed for one job (`--with pymysql`) or with no
wheel for every Python (`duckdb`).

## Inference rules

Operators, functions, aggregates and JSON accessors are typed by `*.inference.toml`
files in `rust/crates/sqlty-sql/rules/`, compiled by `build.rs` into static match arms.
Type a new function or operator with a rule, not in Rust: a rule carries its reference
and reasoning, and `--explain` shows it. When a result needs code (it depends on an
argument's value, say), add a `@` result kind to `compile.rs` and cite the rule.
[`rules/README.md`](rust/crates/sqlty-sql/rules/README.md) has the format, the
lookup order and how to verify a `reference`. Every rule needs a `reference` to
vendor documentation of the version the tests target, one per database
(`reference.postgres` for PostgreSQL 17, `reference.mysql` for MySQL 8.4,
`reference.sqlite` for SQLite 3, `reference.duckdb` for DuckDB 2.0), checked against the published page, and a one-sentence `reasoning`;
the build fails otherwise. `sqlty check --explain` and `-- rules:` mdtest
assertions show which rule typed a column; every rule needs one, in
`tests/mdtest/type_inference/rule_coverage.md` if nowhere else
(`test_every_inference_rule_is_asserted`). `rule_dispatch_does_not_allocate`
(`analyzer/inference.rs`) keeps rule lookup free of heap allocations.

## Agent Skills

`skills/<name>/SKILL.md` (and its `references/`) are the Agent Skills, installed
through the plugin manifests (`.claude-plugin/`, `.codex-plugin/`,
`.cursor-plugin/`) or `sqlty skill --install`. `rust/crates/sqlty-cli/build.rs`
compiles them into the extension: rebuild it after editing one, or
`test_agent_skills_are_compiled_in` fails. A new skill gets a row in the table of
`docs/recipes/agents.md`.

## Check budget

SQL and Python type inference have separate budgets. SQL inference must be faster than a
round trip to the database, or asking the database (as
[sqlx](https://github.com/launchbadge/sqlx) does at compile time) would be the better
checker. The round trip is PostgreSQL typing the same query (Parse and Describe)
over a Unix socket: about 100 µs on a machine whose `bench_project.py` calibration loop
takes 20 ms. Real setups are slower: 126 µs (socket) and 211 µs (TCP) on a loaded laptop
([Cybertec](https://www.cybertec-postgresql.com/en/?p=4197)), 230-400 µs within one
cloud zone ([Bailis](https://bailis.org/blog/communication-costs-in-real-world-networks/)).

Python inference (reading row types, wrappers, calls) must be no slower than
`ruff check` of the same files on one thread: it parses with ruff's parser and makes one
pass over the module, as a lint does.

| Inference | Measured as | Budget | Guarded by |
|-----------|-------------|--------|------------|
| SQL, per query | `analyze_select` + `analyze_params`, schema loaded | 100 µs | `bench-ci` (fails) |
| Python, per file | `check_source` minus the SQL of its queries, schema loaded | `ruff check` per file | `bench-ci` `python_us_per_file` mean (fails) |

Cost grows at most linearly with input size: files, queries, tables, columns, statements,
nesting and line length. No `Vec::contains` dedupe, no clone of a table per statement,
no rescan of the source per query; use a `HashSet`, mutate in place, index once. Prove
it by doubling the input up to 32x and comparing the time.

## Completion budget

Completion runs on every keystroke against schemas of 500+ tables, so no per-keystroke
operation may grow with the number of tables N. Tests count the catalog entries a
lookup reaches (`catalog::visited`), not time, and fail when a path starts scanning the
catalog: `lookups_do_not_depend_on_the_number_of_tables` (`catalog/index.rs`) and
`work_does_not_depend_on_the_number_of_tables` (`analyzer/complete.rs`) compare 50, 540
and 5000 tables.

| Operation | Allowed cost | Guarded by |
|-----------|--------------|------------|
| Locate the query (`query_at`) | O(log S) per request, O(file) once per buffer version | Criterion `query_at`, CodSpeed `test_completion_in_a_large_module` |
| Table prefix | O(log N + k), k capped | entry counts, Criterion and CodSpeed at 50/540/5000 tables |
| Tables linked after `JOIN` | O(K + log F + k) per table in scope, for its K foreign keys and the F of the schema | entry counts |
| `alias.` columns | O(1) + O(width of that table) | entry counts, Criterion and CodSpeed |
| Unqualified column | O(sum of the widths of the tables in scope) | entry counts, Criterion and CodSpeed |
| Index build | O(N log N + F log F) per schema load, two pointers per name and three words per foreign key | `index_shares_the_catalog_keys`, `bench-ci` `schema_load_5000` RSS |
| Per keystroke | never O(N), never waiting behind a check | `completion_is_answered_while_a_check_runs` (`sqlty-lsp`) |

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
UNTYPED`, `from sqlty import NoParams`, or the configured `import-style` line). `check --fix` is the exception: its
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
(user docs, also the Zensical site; `docs/reference/rules/` has one page per lint rule, also
printed by `sqlty rule NAME`). Every test runs through `sqlty.core`.

### Where to add tests

User-visible behavior belongs in `docs/guide/` too; edge cases only in
`tests/mdtest/`. A new lint rule is its page `docs/reference/rules/<name>.md`, with the front matter
(`order`, `group`, `severity`, `fixable`, `summary`, `origin`) that is the only place its metadata
is written, and its check code. The `sqlty-rules` crate generates `Rule` and the Python `RULES`
from the pages, and `build.rs` fails on a page that breaks the format; `make generate-docs` adds
its row to the table of `docs/reference/rules.md`.

| Feature area | Directory | Example file |
|-------------|-----------|-------------|
| Type inference | `tests/mdtest/type_inference/` | `basic_types.md` |
| SQL dialects | `tests/mdtest/postgres/`, `mysql/`, `sqlite/`, `duckdb/` | `select.md` |
| Query shapes across dialects | `tests/mdtest/corpus/` | `outer_joins.md` |
| Nullability | `tests/mdtest/nullability/` | `inference.md` |
| Rewriting | `tests/mdtest/rewrite/` | `basic_rewrite.md` |
| Stripping | `tests/mdtest/strip/` | `basic_strip.md` |
| Formatting | `tests/mdtest/format/` | `format.md` |
| Lint rules | `tests/mdtest/check/` | `type_annotation.md` |
| Magic codec | `tests/mdtest/codec/` | `basic_codec.md` |
| `.sql` query files | `tests/mdtest/queries/` | `named_queries.md` |
| Completion | `tests/mdtest/completion/` | `completion.md` |
| Hover | `tests/mdtest/hover/` | `hover.md` |
| Highlighting | `tests/mdtest/highlighting/` | `highlighting.md` |

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
  In a ```` ```sql migration ```` block, `-- error:` asserts what the migration
  rules report for the block's statements that `against.sql` does not have
  (`_core.check_migrations()`).
- In a ```` ```sql schema ```` block, `-- error:` asserts what `sqlty check` reports on
  the block as the schema itself (`migrations::check_schema`), such as `tenant-policy`.

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
| `-- inferred:` | in the `sql` block | `analyzer::infer_type` |
| `-- expanded:` | in the `sql` block | `analyzer::expand_star` |
| `-- columns:` | in the `sql` block | `analyzer::analyze_select`: `name: type (SQL_TYPE)` per line |
| `-- json:` | in the `sql` block | `analyzer::analyze_select`: `name: shape` per JSON column of known shape |
| `-- rules:` | in the `sql` block | `analyzer::explain_select`: `name: type-rule-id / nullability-rule-id` per column, `-` for none |
| `-- params:` | in the `sql` block | `analyzer::analyze_params`: `number-or-name: type (SQL_TYPE)` per parameter, then ` NULL` or ` NOT NULL` when known |
| `# rewritten:` | `python`, whole code | `sqlty rewrite` without imports |
| `# rewritten-file:` | `python`, whole code | `sqlty rewrite` |
| `# stripped:` | `python`, whole code | `sqlty strip` |
| `# fixed:` | `python`, whole code | `sqlty check --fix` |
| `# decoded:` | `python`, whole code | magic codec `decode()` without imports or the `# coding:` line |
| `# completions:` | `python`, at its `<CURSOR>` | `locate::complete`: `label kind detail` per item, the first ten then `...`; `None` for none |
| `# hover:` | `python`, at its `<CURSOR>` | `locate::hover`: the Markdown contents; `None` for none |
| `# highlighted:` | `python`, whole code | `Queries::highlights`: `kind text` per colored token, a line each; `None` for none |
| `# error: [rule] "message"` | end of its line, or on the line above | `sqlty check`; `sqlty breaking` with an `against.sql` |

- Consecutive comments of one kind form a multi-line value.
- `# error:` takes a rule, a message substring, or both. A test with any
  `# error:` fails on every diagnostic it doesn't assert; a bare `# error:`
  asserts there are none.
- `# error[9:19]:` also asserts the columns the diagnostic underlines (`start`
  and `end`): 1-based characters, the end one past the last. A column on another
  line than the diagnostic's is written `+1.5`. `make update-mdtest` drops ranges.

Each crate runs the assertions it implements, from `rust/mdtest/`: sqlty-sql the
SQL ones, sqlty-format `# formatted:` and the `sqruff` rule's diagnostics,
sqlty-python the rest, and the SQL ones of `driver` sections. pytest runs
`# decoded:`, which needs the Python codec.

### Workflow

1. Write the code with empty assertions (`# rewritten:`, `# error:`).
2. `make update-mdtest` fills them in; errors are written above their lines.
3. Review the diff, then commit.

`MDTEST_FILTER=check/user_types cargo test -p sqlty-python --test mdtest` (in `rust/`)
reruns one file; `MDTEST_UPDATE=1` writes the actual values.

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

- `tests/test_core_invariants.py` checks idempotency and round trips over every
  mdtest code block: new mdtests extend it automatically.
- `docs/reference/configuration.md`, `cli.md` and `rules.md` hold tables between
  `<!-- generated:NAME -->` markers, rendered by `scripts/generate_docs.py` from `KEY_DOCS` in
  the script itself, the clap commands of `sqlty-cli` and the rule pages. Edit those, then run
  `make generate-docs`; `test_generated_reference_is_current` fails on drift.
- `tests/test_docs.py` checks that every rule has a page in `docs/reference/rules/`, that
  relative links resolve, that `examples/` need no rewrite, and runs the
  `>>>` examples in `docs/reference/python-api.md`.
- Focus on **motivated examples** — show the problem first, then the solution.
- Always show complete working code, not snippets.

## Type checker support

`src/sqlty/` and the examples must pass both **mypy** (strict) and **ty**
with zero errors (`make typecheck`). `SQL[T]` usage is additionally checked with mypy,
pyright and ty by `tests/test_typecheckers.py`.

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

## No exec

`check` reads row types from source. It never executes, evaluates or imports what a
checked file names, so checking an untrusted pull request is safe. A harness keeps it so:

| Guard | Where | Catches |
|-------|-------|---------|
| Static scan | `tests/test_no_exec.py` | A call in `src/sqlty` to `exec`, `eval`, `compile`, `__import__`, `importlib.import_module`, `get_type_hints`, `types.new_class` and the like, outside `tests/no_exec_allowlist.txt` |
| Audit hook | `tests/_no_exec.py`, loaded for every test | At runtime, any `exec`/`eval` or import that sqlty code runs, charged to the innermost `src/sqlty` function |
| No I/O in the engine | `rust/crates/sqlty-sql/clippy.toml` (`sqlty-python` links it), run by `cargo clippy` | A file, process, network or standard stream call in `sqlty-sql` or `sqlty-python`; a test or `build.rs` that needs one says so with `#[expect(clippy::disallowed_methods, ...)]` |
| Integrity check | `scripts/check_no_exec_harness.py`, CI job `No-exec harness intact`, `make check-no-exec-harness` | Against the merge base: guard files edited or unloaded, allowlist lines added, mdtest assertions removed or edited (a bare `# error:` may become specific; a copy may go while a block with the same setup and code still asserts it), skips or xfails added, test functions removed |

CI runs the base branch's copy of the integrity check, so a pull request can't relax it.
The allowlist is empty: never add a line. A change to a guard file fails the check by
design and needs a maintainer to merge it.

Audit hooks are not a sandbox: code can bypass them
([`sys.addaudithook`](https://docs.python.org/3/library/sys.html#sys.addaudithook)).
They catch mistakes; review catches intent.
