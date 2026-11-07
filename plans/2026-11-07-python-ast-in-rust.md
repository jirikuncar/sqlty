# Python AST passes in Rust

> **Status:** milestone 1 on branch `rust/python-ast-migration`.

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
2. Driver and t-string calls.

## Open questions

- Raise the MSRV from 1.88 to 1.97.
- WebAssembly size with the parser in `sqlty-wasm`; it gains every AST check.
- `_variants.py` evaluates real `psycopg.sql` classes to render compositions; in Rust it
  needs its own model of `SQL`, `Identifier`, `Literal` and `format`.
