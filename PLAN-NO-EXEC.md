# Plan: resolve row types statically, without `exec`

> **Status:** harness merged first; implementation not started.
> Owner: @jirikuncar. Every pull request of this plan links it.

## Goal

`sqlty check` reads row types (`TypedDict`, `NamedTuple`, dataclass, `BaseModel`,
`Protocol`) from source only. Like ty, mypy and pyright, it never executes, evaluates
or imports anything a checked source names: no `exec`, `eval`, `compile(..., "exec")`,
`typing.get_type_hints`, `ForwardRef` evaluation or user-driven import. What `check`
reports stays the same.

## Where `exec` is today

All of it is in `src/sqlty/_type_extractor/sandbox.py`, behind `extract_type_definitions`:

| Function | What it runs |
|----------|--------------|
| `_populate.<locals>.flush` | `exec` of the guarded module-level imports, aliases and `NewType`s (also of helper modules, via `_module_namespace`) |
| `_resolve_class` | `exec` of the class definitions |
| `_type_hints` | `annotationlib.get_annotations` + `typing.get_type_hints` (evaluates annotation strings) |
| `_own_extra_type` | `typing.evaluate_forward_ref` of PEP 728 `extra_items` |

Imports run only for the standard library, `_TRUSTED_MODULES` (`typing`,
`typing_extensions`, `pydantic`, `pydantic_core`, `annotated_types`, `sqlty`) and the
`type-modules` setting.

A second, static path already exists: `ast_fields._ast_type` builds the same `typing`
objects from the AST (`field_types`), and `compat` falls back to it. Measured on
`e06dee7` with the sandbox disabled (`make test-static-only`):

- 996 tests reach `exec`; 26 of 9,866 fail without it.
- 19 of the 26 assert sandbox internals (`resolved_hints`, `type-modules` importing):
  `tests/test_type_extractor.py` (18) and the doctest of `_type_extractor.generic`.
- 7 are real gaps: cross-module `NewType`/alias chains and re-exports
  (`tests/test_cross_module.py`, `generic_fields.md`).
- 10 more behaviors had no test at all: local aliases (`X = int`, `type X = int`,
  `X: TypeAlias = int`), alias of an alias, `NewType` of an alias, string annotations,
  forward references to a later alias, and standard-library classes outside the scalar
  table (`pathlib.Path`, `ipaddress.IPv4Address` as an attribute, `fractions.Fraction`).
  `tests/mdtest/check/static_resolution.md` now pins them; 12 of its 16 cases fail
  without the sandbox, so `make test-static-only` starts at 38 failures.

## Approaches

| | Approach | Verdict |
|-|----------|---------|
| A | Extend the static path (`_ast_type`) with a per-module symbol table and string annotations; delete the sandbox. `compat`/`subtype` keep working on `typing` objects. | **Recommended.** Smallest diff, already 99.7% of the suite, no new dependency. |
| B | Own symbolic type model (`ClassRef`, `Union`, `NewTypeRef`, `Unknown`, ...) as ty's `Type`, porting `subtype`/`compat` to it. | Cleaner, and a path to Rust later. Twice the diff for no user-visible gain now; revisit if A's `typing` objects get in the way. |
| C | Embed ty: `ty_python_semantic` (and `ruff_python_parser`) are on crates.io, but as 0.0.x "internal component crate[s] of Ruff", released weekly. | Rejected: no stable API, Salsa and a large dependency tree in every wheel, for a feature that needs a few dozen classes. |
| D | Ask the `ty` CLI (generated probe file, parse its diagnostics). | Rejected for `check`: needs ty installed, output not a stable interface, a subprocess per file. Useful as a test oracle (see [Optional oracle](#optional-oracle)). |

Type checkers resolve imports from the filesystem in this order: stubs or sources at the
start of the path, user code, typeshed's standard library, `-stubs` packages, packages
with `py.typed`, vendored third-party stubs
([typing spec](https://typing.python.org/en/latest/spec/distributing.html#import-resolution-ordering));
ty searches first-party paths, then `PYTHONPATH`, then the environment's `site-packages`
([ty docs](https://docs.astral.sh/ty/modules/)). sqlty needs the first two only: the
checked paths (`HelperIndex`) and a table of the standard library.

## Design (approach A)

1. **Symbol table.** `ast_fields` gets `_symbols(tree) -> dict[str, Symbol]` for the
   module-level names a type can go through: `X = <type expr>`, `X: TypeAlias = ...`,
   `type X = ...` (type parameters make it `Unknown` when subscripted), `NewType`,
   classes (by name only), and imports (`import a.b as c`, `from .x import y`).
   Resolution is lazy, memoized per `(module, name)` on `HelperIndex`, and cycle-safe
   (a name being resolved is `None`, unknown). It replaces `_namespace_statements`,
   `_populate`, `_module_namespace`, `_ModuleProxy` and `_lookup`.
2. **`_ast_type` takes a resolver** in place of `newtypes`: a `Name` or `mod.Name` goes
   through the symbol table (local, then helper modules, then the standard-library
   table). A string constant is parsed with `ast.parse(s, mode="eval")` and resolved
   the same way. `ast.parse` compiles to an AST only and runs nothing.
3. **Standard-library classes.** Inferred column types are a closed set
   (`info.PYTHON_TYPE_MAP`). A stdlib class matters only through its relation to them.
   `scripts/generate_stdlib_types.py` (run by `make generate-docs`, drift-tested like
   the reference tables) writes `_type_extractor/_stdlib.py`: each public stdlib class
   whose MRO contains a column type (`enum.IntEnum`, `collections.OrderedDict`, ...),
   with those bases. Any other stdlib class resolves to an opaque nominal class
   (`type(name, (), {"__module__": module})`, one per qualname), disjoint from every
   column type. That is the runtime behavior for `pathlib.Path`. Non-stdlib modules
   outside the checked paths stay unknown and are never reported, as today.
4. **TypedDict keys.** `__required_keys__` and `extra_items` come from the AST
   (`required`, `_ast_type` of `extra_node`), which already compute them.
5. **`TypeInfo` shrinks.** `resolved_hints`, `declared_hints` and `extra_items_hint`
   merge into `field_types` (`| None` kept) plus `_unwrap_optional` at use. `compat`,
   `generic`, `_check`, `_json_shape` read one source.
6. **Delete** `sandbox.py`, `_Unknown`, `_TRUSTED_MODULES`, the `exec` paragraph of the
   `_type_extractor` docstring and `tests/_static_only.py`.
7. **`type-modules`** has nothing left to do. See [Decisions](#decisions).

## Phases

Each phase is one pull request, green on the full CI and on `No-exec harness intact`.

| # | Scope | Exit criterion |
|---|-------|----------------|
| 0 | Harness (this pull request) | Harness merged; suite green |
| 1 | Symbol table + resolver for one module; string annotations | `make test-static-only`: `static_resolution.md` sections *Local aliases* and *String annotations* pass |
| 2 | Helper modules through the symbol table | `make test-static-only`: `test_cross_module.py`, `generic_fields.md`, *Other modules* pass |
| 3 | Generated stdlib table | `make test-static-only`: *Standard library classes* pass; drift test green |
| 4 | Delete the sandbox; collapse `TypeInfo`; rewrite the 19 internal tests to assert behavior or `field_types`; `type-modules` per decision; docs | [Definition of done](#definition-of-done) |

In phases 1 to 3 the sandbox still runs, so `make test` stays green and only
`make test-static-only` shows progress. Fill the gap there; never patch the
sandbox to match.

## Harness

What keeps an implementation (human or agent) from widening scope, weakening tests or
hiding `exec` behind another name:

| Guard | Where | Catches |
|-------|-------|---------|
| Static scan | `tests/test_no_exec.py` | Any call in `src/sqlty` to `exec`, `eval`, `compile`, `__import__`, `breakpoint`, `importlib.import_module`/`reload`, `runpy.*`, `exec_module`/`load_module`, `get_type_hints`, `evaluate_forward_ref`, `get_annotations`, `call_evaluate_function`/`call_annotate_function`, `types.new_class` outside `tests/no_exec_allowlist.txt`, and stale allowlist lines (so it only shrinks). |
| Audit hook | `tests/_no_exec.py`, loaded for every test (`pyproject.toml` `addopts`) | At runtime, any `exec`/`eval` or import that sqlty code (or code it executed) runs, charged to the innermost `src/sqlty` function, outside the allowlist. sqlty's own `import` statements and module bodies are free. 996 tests trip it with an empty allowlist. Self-tested by `test_guard_catches`. |
| Behavior lock | `tests/mdtest/check/static_resolution.md` and every existing mdtest | Diagnostics must not change. |
| Integrity check | `scripts/check_no_exec_harness.py`, CI job `No-exec harness intact`, `make check-no-exec-harness` | Against the merge base: guard files unchanged and still loaded; allowlist only shrinks; no mdtest assertion removed or edited (a bare `# error:` may become specific); no new skip/xfail/`python ignore`; no test function removed. CI runs the **base branch's** copy, so a pull request cannot relax it. |
| Static-only run | `tests/_static_only.py`, `make test-static-only` | Measures the static resolver with the sandbox disabled. |

Audit hooks are not a sandbox: code can bypass them
([`sys.addaudithook`](https://docs.python.org/3/library/sys.html#sys.addaudithook)).
They catch mistakes; CODEOWNERS review catches intent.

### Rules for implementors

- Work only in `src/sqlty/_type_extractor/`, `src/sqlty/_helpers.py`, the call sites that
  pass `type_modules`, `scripts/generate_stdlib_types.py`, and the docs and tests named
  here. Anything else is a separate pull request.
- Never edit `tests/_no_exec.py`, `tests/test_no_exec.py`,
  `scripts/check_no_exec_harness.py` or existing mdtest assertions, and never add
  lines to the allowlist. If one looks wrong, stop and ask.
- Remove an allowlist line in the same commit as the code it names.
- `make update-mdtest` may only fill new, empty assertions. Review its diff: a
  changed existing line is a regression.
- New behavior gets cases in `static_resolution.md` first, filled from the sandbox
  (phases 1 to 3), then made to pass statically.
- Per AGENTS.md: `uv run prek run --all-files` before each commit, no force-push, and
  disclose the agent in the commit trailer and the pull request.

### Optional oracle

For the `TypedDict` and `NamedTuple` cases of `static_resolution.md`, a test can
generate `def f(x: <column type>) -> <annotation>: return x` and check that ty reports
`invalid-return-type` exactly where sqlty reports `column-type-mismatch`. ty is
already a dev dependency. Pydantic lax mode, the `int` -> `float` promotion and NULL
handling are known differences and are excluded.

## Definition of done

1. `tests._no_exec.sites()` is empty: no call of the scanned list remains in `src/sqlty`.
2. `tests/no_exec_allowlist.txt` holds its header only.
3. `src/sqlty/_type_extractor/sandbox.py` and `tests/_static_only.py` are deleted;
   `grep -rn "_TRUSTED_MODULES\|_Unknown\|resolved_hints\|declared_hints" src tests` is
   empty.
4. `uv run pytest` is green, including every assertion of `static_resolution.md` as
   pinned in phase 0, and `make check-no-exec-harness` passes against `main`.
5. `test_application_imports_are_not_executed` holds for every configuration,
   `type-modules` included. A CLI test runs `sqlty check` on a project whose
   imported module writes a marker file, and no marker appears.
6. `type-modules` handled per [Decisions](#decisions); `docs/guide/row-types.md`
   ("`check` never runs the imports...") and `docs/reference/configuration.md` describe
   the static behavior; `make generate-docs` shows no drift; `make lint-docs` is clean.
7. `make typecheck` and `uv run prek run --all-files` are clean.
8. `make bench-ci` stays within `benchmarks/baseline.json`; CodSpeed shows no
   regression on `check_source`. A speed-up is expected: `pydantic` is no longer
   imported.
9. AGENTS.md (architecture) and the `_type_extractor` docstring describe one static
   path. This plan moves to `PLAN-FUTURE.md` or is deleted.

## Decisions

Open, with the recommendation first:

1. **`type-modules`**: (a) keep accepting it with a deprecation warning for one minor
   release, then remove it; (b) remove it now (an unknown key is an error, so configs
   that set it break); (c) keep it as a silent no-op.
2. **Classes of the standard library**: (a) generated table plus opaque nominal classes,
   as in the design; (b) curated table only, other stdlib classes unknown, which
   drops the `pathlib.Path` and `fractions.Fraction` diagnostics pinned in phase 0 and
   needs an explicit exception to the behavior lock.
3. **User classes deriving from builtins** (`class Email(str)`, `enum.StrEnum`): no
   diagnostic today, even with the sandbox. (a) keep parity in this plan and tighten
   in a follow-up; (b) tighten here, adding assertions only.
