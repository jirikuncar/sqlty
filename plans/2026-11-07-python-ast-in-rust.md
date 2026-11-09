# Python AST passes in Rust

> **Status:** milestones 1 to 4 on branch `rust/python-ast-migration`.

The checks that need a Python syntax tree run in Python on `ast`: helper calls
(`_helpers.py`), driver calls (`_drivers.py`), `psycopg.sql` compositions (`_psycopg.py`),
query variants (`_variants.py`), parameter literals (`_params.py`) and row types
(`_type_extractor/`), about 5,000 lines over one `ParsedSource` scan (`_source.py`).
`sqlty-python` has a tokenizer and no tree, so none of this runs in the WebAssembly
playground or the language server without Python.

## Where the time goes

`sqlty check -j 1` over 10,000 synthetic files (`bench_project.py --size full`, release
build, Python 3.15), 3.3 s in all:

| Part | Time |
|------|------|
| Project setup (walk, settings, module names, helper index) | 0.7 s |
| `ast.parse` | 0.6 s |
| Rust (`check_source`, `wrapped_queries`, analysis) | 0.5 s |
| `_Scan` and the Python passes | 1.3 s |
| Extension load, schema, output | 0.2 s |

`ast.parse` is about a third of the per-file Python work, and every AST pass needs it.
Moving one pass alone keeps the parse: driver detection, for example, is 0.2 s of a
psycopg-heavy run. The gain comes from moving the parse with every pass that reads it.

## Parser

[`ruff_python_parser`](https://crates.io/crates/ruff_python_parser), published by Astral
from the Ruff repository, 0.0.21 (2026-11-05), weekly releases. Its README says the API
"is unstable and will have frequent breaking changes". It needs Rust 1.97 (the workspace
is on 1.88) and brings `ruff_python_ast`, `ruff_text_size` and `ruff_python_trivia`.
Pin an exact version and bump it on purpose. [Monty](https://github.com/pydantic/monty) does
the same: it pins every `ruff_*` crate at one version (0.0.14, `rust-version` 1.98) and
lowers the ruff AST into its own tree in `crates/monty/src/parse.rs`, so only that file
sees ruff's types. Stage 1 follows it.

## Stages

Each stage keeps the mdtests as the specification and runs both implementations on the
mdtest corpus and the synthetic project until they agree.

| Stage | What | Python left |
|-------|------|-------------|
| 1 | Parser in `sqlty-python`, a `Module` scan (imports, calls, scopes, bindings, line index); MSRV 1.97 | none removed |
| 2 | Driver and t-string calls (`find_driver_calls`, `find_template_calls`), parameter literals | `_drivers.py`, most of `_params.py` |
| 3 | Marked and configured helpers with the cross-file helper index | `_helpers.py` |
| 4 | `psycopg.sql` compositions and query variants | `_psycopg.py`, `_variants.py` |
| 5 | Row types (`_type_extractor`); `ParsedSource` and `ast` gone from `check` | `_source.py` |

The parse is saved only once stage 5 lands; stages 2 to 4 each move a pass but parse twice
in between, unless stage 1 hands Python the facts it needs instead of a tree.

## Switch

`SQLTY_AST` picks who reads the tree: `python` (the default, `ast` only), `rust` (Rust first,
`ast` where Rust cannot decide), and `compare`, which runs both and raises `AstMismatch` when
they differ. CI runs the mdtests in `compare`; `tests/test_module_facts.py` compares the two
readings on generated modules (Hypothesis) and on every Python file and mdtest block.

## Milestones

Slowest first, each measured on the 10,000-file synthetic project.

1. `sqlty_python::module::facts`: the called names, `from` imports and `X[Any](...)` calls.
   `find_helper_calls` skips a file whose calls name no indexed helper, and `any_calls` reads
   the Rust offsets, so a file that imports helpers but never calls one is never parsed by
   `ast`. `check -j 1`: 2.88 s to 2.22 s on the full project; 3.85 s to 4.13 s (median, noisy)
   when every file calls a helper, since those files are parsed twice.
2. `sqlty_python::drivers`: `find_driver_calls`, with its scopes, bindings and argument
   types, and `typing_aliases`. A gate resolving `f(...)` through the helper index skipped
   files whose calls name no helper; milestone 3 removed it, as it missed `pkg.alias(...)`
   of a helper a package re-exports under another name. `check -j 1`, median of 3: 4.15 s to 2.55 s when every file calls a psycopg helper, of
   which 880 of 10,000 files still reach `ast` (type definitions, helper calls); 2.95 s to
   2.28 s on the full project. Output is identical in both modes.
3. `sqlty_python::helpers`: the calls `find_helper_calls` resolves, with their arguments
   (string literal, type, literal parameters, span) and the class of a method's receiver.
   Python still resolves them against the helper index and parses only the row argument,
   at its own position (`_expression_at`). A file that defines helpers, or passes a helper
   anything but a string literal (a name, a composition, a forwarded parameter), still goes
   to `ast`. `check -j 1`, median of 3 on a slower day: 4.11 s to 3.31 s on the full project
   (1,303 files no longer reach `ast` for helper calls; 534 still do for type definitions),
   4.83 s to 3.81 s when every file calls a psycopg helper. Output is identical.
4. Type definitions and t-string calls, with no `ast` fallback: where Rust declines a file,
   it has no types and no t-string calls. `ParsedSource.outline` is the module as `ast`
   nodes built in Rust from the ruff tree (`sqlty-ext/src/outline.rs`), function bodies cut
   to the statements holding a class; type extraction, `type_aliases` and generic rows read
   it and its classes. `sqlty_python::templates` reads the `--sql` t-strings, and
   `imported_names` and the helper index imports come from the Rust facts. On 2,918 files
   of the standard library and installed packages the outline dumps the same as `ast.parse`
   cut the same way, positions included; `tests/test_outline.py` and
   `tests/test_template_strings.py` compare generated modules. One of 10,000 files still
   reaches `ast.parse` (it defines helpers). `check -j 1`, median of 3: 3.95 s to 3.20 s on
   the full project, 5.00 s to 3.58 s when every file calls a psycopg helper; output is
   identical. Building the outline costs about 18 µs a file, against 26 µs for `ast.parse`.
5. Compositions and query variants.

## Where ruff and `ast` differ

`sqlty_python::parse` returns `None` wherever the two parsers can disagree: Python's `ast`
decides in milestones 1 to 3, and from milestone 4 the file has no types or t-string calls. Each case is in `parse.rs` tests or `tests/test_helper_sites.py`.

| Source | `ast.parse` | ruff 0.0.21 | Rust does |
|--------|-------------|-------------|-----------|
| Nested 10,000 deep (brackets, `-`, `not`, `lambda`, `**`, `if`/`else`) | `SyntaxError`, `MemoryError` | overflows the stack and kills the process | parses on a stack of 2 KiB per byte (files up to 256 KiB), defers past depth 1,000 |
| More than 200 open brackets; 100 indentation levels | `SyntaxError`, `IndentationError` | accepts | defers |
| 150 nested f-strings; a format spec three deep | `SyntaxError` | accepts | defers |
| `elif` chain of 10,000 | `MemoryError` (each `elif` nests) | accepts (flat) | counts each `elif` as a level |
| Leading byte order mark (`check_source` on a `str`) | `SyntaxError` | accepts | defers |
| NUL character in a string | `SyntaxError` | accepts | defers |
| `from __future__ import braces` / unknown feature at the top | `SyntaxError` | accepts | defers; also for `barry_as_FLUFL` |
| `'\ud800'` | keeps the surrogate | U+FFFD | defers on any `\uD8xx`-`\uDFxx` escape |
| A file over 256 KiB | reads | reads, on a stack sized by length | declines: no types or t-string calls |
| `\r\n` or `\r` inside a string (triple-quoted, CRLF file) | `\n` | kept as written | reads the literal again with `\n` (`parse::text`); the outline normalizes the file first |
| Python 3.15 syntax (`lazy import`, `[*x for x in y]`) | accepts | accepts (lazy imports are imports) | reads |
| `ｅｘｅｃｕｔｅ(...)` (NFKC) | `execute` | `execute` | same tree; the text gates on raw text skip it in both |

Name resolution matches `find_helper_calls`, and the corner cases are tested as such:
resolution is by imports only, so a local `def` or assignment does not shadow an imported
helper; of two imports of one name the later in `ast.walk` (breadth-first) order wins,
whichever branch would run; a function-level import binds the name file-wide; star imports,
`importlib` and `__import__` are not followed; relative imports need the module name. No
module is ever imported, so imports have no side effects. Two behaviours of the Python pass
are kept rather than fixed: a file is looked at only when its text names a helper as
defined (a package re-export under another name alone is missed), and a call in a function
or class whose source never names a helper is skipped unless an earlier pass walked every
call of the file (`any_calls`); Rust always skips it.

Two Python behaviours changed so both readings agree on the cut outline: a name bound only
in a function body nested in a module-level statement is no longer a module binding for
type aliases (`_bound_names`), and t-string calls come in source order, not `ast.walk` order.

## Open questions

- WebAssembly size with the parser in `sqlty-wasm`; it gains every AST check. On wasm32
  `stacker` allocates the parse stack on the heap, so the per-byte stack needs a lower cap.
- Each file is parsed by ruff up to four times (facts, outline, t-strings, helper sites); one
  parse kept between the calls would save about 10 µs a call.
- The surrogate escapes and the 256 KiB limit cost a file its types in `rust` mode: decoding
  string escapes in Rust, and a stack sized by nesting rather than length, would lift both.
- `_variants.py` evaluates real `psycopg.sql` classes to render compositions; in Rust it
  needs its own model of `SQL`, `Identifier`, `Literal` and `format`.
