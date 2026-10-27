# Query variants from branches

> **Status:** stage 1 implemented behind `preview = true` (`src/sqlty/_variants.py`).
> Acceptance spec: `tests/mdtest/check/query_variants.md`.

## Problem

A query assembled from names bound in branches is not checked today:

```python
var: LiteralString
if foo is None:
    var = "foo IS NULL"
else:
    var = "foo = %(foo)s"
fetch_all(sql.SQL("SELECT id, foo FROM foos WHERE {}").format(sql.SQL(var)), tuple[int, str])
```

`PsycopgScan.binding` (`src/sqlty/_psycopg.py`) follows a name only when it has exactly
one assignment in its scope; two make the query `unchecked-query`. A plain `LiteralString`
query that is not a literal at the call is `dynamic-query` (off by default), so the
call above passes silently.

Two notes on the example as first written:

- `sql.SQL("...{}").format(var)` with a `str` renders `var` as a quoted value: psycopg
  wraps any non-`Composable` argument in `sql.Literal`
  ([psycopg docs](https://www.psycopg.org/psycopg3/docs/api/sql.html)). It needs
  `sql.SQL(var)`.
- `.format()` reads fields of its own template only, so a `{foo}` inside a substituted
  `sql.SQL` stays as text. Use a placeholder (`%(foo)s`) or format the fragment first.

## Goal

Check every query a function can build from literals, without running user code:
enumerate the **variants** (one rendered SQL string per feasible path) and run the existing
checks on each.

## Prior art

- SQLFluff renders Jinja templates in several variants to lint unreached `{% if %}`
  branches, capped by `render_variant_limit` (default 5 since 4.2), best effort
  ([docs](https://docs.sqlfluff.com/en/stable/configuration/templating/variants.html)).
  sqlty differs: a variant cap is reported, never silently truncated.
- psycopg `Composable.as_string(None)` renders without a connection, which
  `_psycopg.py` already uses.

## Approaches

| # | Approach | Correlation kept | Cost | Verdict |
|---|----------|------------------|------|---------|
| A | Every assignment of a name in scope is a candidate; cartesian product | no | small | rejected: false positives on correlated names (spec: "Names bound in one branch stay together") |
| B | Reaching definitions over a CFG, product at the use | no | medium | rejected: same false positives, needs a CFG |
| C | Structured path enumeration: walk the enclosing function body, fork an environment at `if`/`match`/`IfExp`, drop paths that `return`/`raise`, join at the use | yes | medium | **chosen** |
| D | Ask a type checker (pyright/ty) for `Literal[...]` types at the use | no | large: external process, per-checker behaviour | rejected; worth a spike to confirm what pyright and ty infer at a join |
| E | Execute the code with mocked inputs | yes | - | rejected: sqlty never runs user code |

C fits Python: the AST is structured (no `goto`), so paths are a tree walk, not a CFG.
It is demand-driven: only names the query expression reads are tracked.

### What C models

| Construct | Variants |
|-----------|----------|
| `if`/`elif`/`else`, nested | one environment per branch |
| `x = a if c else b` | two |
| `match` with any patterns | one per `case`; no wildcard adds the unchanged environment |
| `return`/`raise`/`continue`/`break` in a branch | path dropped (only at function level, not in loops) |
| `x += "..."` on a `str` | appended per path |
| A parameter annotated `Literal["a", "b"]` | one per member |
| `D[k]` where `D` is a module-level dict of string literals | one per value; per key when `k` is a `Literal` |
| Call of a function of the module (or a checked module) returning `LiteralString` | union of its `return` variants, with the arguments bound to its parameters; depth-bounded |
| Same condition (`ast.dump` equal, none of its names reassigned between) tested twice | same branch both times |

### Static values

Stage 1 requires every branch to bind a static value, built only from:

- a string literal (no f-string or t-string);
- `+` and `+=` of static values;
- a name whose every binding on the path is static.

A branch binding anything else (a call, a parameter, a subscript, an f-string) leaves the
query `dynamic-query`/`unchecked-query`: no variant is checked, the others included.
Later stages widen the set with `Literal` parameters, literal dict lookups and function
returns (rows below).

Anything else touching a tracked name (`for`/`while` body, `with ... as`, walrus,
`global`/`nonlocal`, `del`, `try`, a call mutating it) keeps today's result:
`dynamic-query` or `unchecked-query`. Never a partial check.

### Reporting

- Each variant runs through the existing checks unchanged.
- A diagnostic raised by every variant is reported once, as today.
- Otherwise each distinct diagnostic gets the suffix `(variant: line N[, M...])`: the
  lines of the bindings that select the first failing variant.
- `stale-type`/`narrow-type` and `--fix` compare against the union of the variants' row
  types (column-wise type union, nullable if any variant is). `--fix` acts only when all
  variants have the same column count.
- Parameter checks run per variant when the placeholders and the passed parameters are
  both known on that path; otherwise skipped for that call.
- Over `max-query-variants` (new key, default 64): `dynamic-query`/`unchecked-query` with
  `"<n> variants, more than max-query-variants (<limit>)"`.

## Design

- New `src/sqlty/_variants.py`: `Variants.of(expr) -> list[Variant] | Dynamic`, where
  `Variant = (value, bindings: tuple[int, ...])`. Generalises `_Run.value` to sets;
  `PsycopgScan.render` and the plain-string path both call it.
- `HelperCall.query: str | None` gains `variants: tuple[QueryVariant, ...]`;
  `_check.py` loops over them and merges diagnostics as above.
- `find_helper_calls` evaluates non-literal queries for every module, not only those
  mentioning `psycopg`.
- `max-query-variants` in `_config.py` with `KEY_DOCS`; `make generate-docs`.
- Rust stays untouched: variants are strings, the analyzer already checks strings.

## Phases

Each phase is one commit that keeps the whole suite green and removes the `xfail`
markers of the spec sections it implements.

0. **Spike** (no product code): psycopg notes confirmed on psycopg 3.2.12; record what pyright and ty infer for the example at the join; measure
   `find_helper_calls` on `benchmarks/` to set the perf budget.
1. **Evaluator over static values** (see [Static values](#static-values)): `_variants.py`
   with `if`/`else`, `IfExp`, `+=`, kills, return/raise pruning, cap. Sections: if/else,
   mismatch in one/every variant, reassigned, augmented assignment, cap; "A branch with a
   value that is not static" must keep passing.
2. **match, elif, correlation**: sections match/case, elif, names together, condition twice.
3. **Enumerated inputs**: `Literal` parameters, dict lookup. Sections accordingly.
4. **Functions**: return variants with argument binding. Section: function fragment.
5. **psycopg**: route `_psycopg.py` through the evaluator. Section: psycopg composition.
6. **Docs**: `docs/guide/` page section, `dynamic-query`/`unchecked-query` rule pages,
   `docs/reference/configuration.md`.

## Harness

What keeps an implementation honest, in order of strength:

1. **Frozen acceptance spec.** `tests/mdtest/check/query_variants.md` holds the
   behaviour; its sections are strict `xfail`, so a section that starts passing fails
   until its marker is removed. `tests/test_query_variants_spec.py` hashes the file with
   the markers removed: any other edit fails. Changing the spec is a separate,
   reviewed commit that updates the digest.
2. **No collateral edits.** No existing mdtest assertion, `benchmarks/baseline.json`
   entry or rule default changes. Reviewer check:
   `git diff origin/main --stat -- tests/mdtest docs src/sqlty/rules benchmarks/baseline.json`
   lists only the spec file and the doc pages of phase 6.
3. **Differential oracle** (`tests/test_query_variants_fuzz.py`, Hypothesis, run with
   `SQLTY_FUZZ=1` like `tests/test_fuzz.py`): generate functions from a grammar of
   `if`/`elif`/`match`/`IfExp`/`+=`/`return`/`raise` over boolean parameters and string
   literals; execute the *generated* code (ours, not the user's) for every input
   combination to get the set of concrete queries; assert
   - soundness: every concrete query is among the static variants, or the call is
     reported dynamic/over the cap;
   - precision: with independent conditions, static variants equal concrete ones.
4. **Never runs user code.** A test checks a module whose evaluation would create a
   file (top-level side effect, function body side effect) and asserts the file never
   appears.
5. **Bounded.** Path count, recursion depth (64, as `_Run`) and call depth (8) are
   constants with tests at each limit; exceeding one reports, never truncates.
6. **Performance.** `make bench-ci` within `benchmarks/baseline.json`; a CodSpeed case
   for a 7-branch function; no change on files without branches (benchmarked).
7. **Standard gates.** `uv run prek run --all-files`, `make typecheck`,
   `uv run pytest`, `test_core_invariants.py` (idempotency over the new mdtests).

## Phase 0 findings

- At the join of `if foo is None: var = "foo IS NULL" else: var = "foo = %(foo)s"`,
  pyright and ty infer `Literal['foo IS NULL', 'foo = %(foo)s']`, mypy infers `str`.
- The feature flag is `preview` (default `false`). The mdtest runner turns it on unless a
  section sets `preview = false`, so the whole Markdown suite runs with variants and shows
  they change nothing else; only the `dynamic-query` examples, whose module constant is
  now followed, pin `preview = false`.
- `check_source` on a 7-branch function (128 variants) spends its time in the analyzer
  (one `analyze_columns` per variant); the walk itself is under 5% of it
  (`benchmarks/test_benchmarks.py::test_check_source_query_variants`). With `preview`
  off nothing runs.
- `with` blocks are unmodelled like `try`: a context manager can swallow an exception
  halfway through the block.

## Definition of done

- [x] Every section of `query_variants.md` passes with no `xfail` marker left, and the
      frozen digest still matches.
- [x] Differential oracle passes at `SQLTY_FUZZ_EXAMPLES=5000`.
- [x] A branch binding a value that is not static leaves the query unchecked, all
      variants included.
- [x] Unmodelled constructs (loops, `try`, `with`, walrus, mutation) still report
      `dynamic-query`/`unchecked-query`; no check runs on a partial set of variants.
- [x] Diagnostics follow the reporting rules above, wording included.
- [x] `max-query-variants` documented (`KEY_DOCS`, `make generate-docs`), validated,
      default 64.
- [x] No existing mdtest, rule default or benchmark baseline changed.
- [ ] `make bench-ci` within baseline; CodSpeed shows no regression on existing cases.
- [x] mypy strict and ty clean; prek hooks clean; full `uv run pytest` green.
- [x] `docs/guide/` explains variants with a motivated example; rule pages of
      `dynamic-query` and `unchecked-query` updated.
- [x] Phase 0 findings recorded in this file.

## Open questions

- Should `dynamic-query` default to `warn` once variants cover the common cases?
- Should the suffix name the condition (`if foo is None`) instead of binding lines?
- Interprocedural evaluation across modules: reuse `HelperIndex.fragment`, or stop at
  the module boundary in the first version?
