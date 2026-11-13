# Playground: share links, configuration, fixes and breaking changes

> **Status:** stages 1 to 4 and 6 merged in [#895](https://github.com/jirikuncar/sqlty/pull/895),
> [#899](https://github.com/jirikuncar/sqlty/pull/899), [#900](https://github.com/jirikuncar/sqlty/pull/900),
> [#901](https://github.com/jirikuncar/sqlty/pull/901) and [#902](https://github.com/jirikuncar/sqlty/pull/902);
> `param-types` in `sqlty.toml` and `examples/06_param_types/` in [#917](https://github.com/jirikuncar/sqlty/pull/917). Stage 5 (breaking
> changes) waits for the comparison of `_breaking.py` to move to Rust.

Tracks [#893](https://github.com/jirikuncar/sqlty/issues/893): what keeps the Playground page
from being the place to reproduce a bug or show a feature. Every stage keeps the page static:
no server, nothing leaves the browser.

## Stages

| Stage | What | Where |
|-------|------|-------|
| 1 | Share links in the URL hash and a `sqlty.toml` panel (`dialect`, `import-style`, `rules`) | `sqlty-wasm` `config` field, `share.js`, `playground.js` |
| 2 | Quick fixes: a `fix` op over `sqlty_python::fixer`, code actions on diagnostics, **Fix all**, and quick actions to rewrite and strip | `sqlty-wasm`, `playground.js` |
| 3 | Examples: presets from `examples/` and the docs, each a share link the docs can open | `playground.js`, docs pages |
| 4 | Output views: the JSON of `sqlty check --output-format json` and the `--explain` text | `sqlty-wasm`, `playground.js` |
| 5 | Breaking changes: a "Before" schema and a `breaking` op, once the comparison is in Rust | `sqlty-python`, `sqlty-wasm` |
| 6 | A Playwright test of the deployed page and of `zensical serve` under `/sqlty/`; `.wasm` size in CI | `docs.yml`, `tests.yml` |

## Stage 1

- The request takes `config`, a `sqlty.toml`, validated by `sqlty_python::config::load` as
  `sqlty check` validates it. Its `dialect` wins over the select. A key that names files
  (`schema`, `overrides`, ...) is an error: the page has no files to read.
- The session (`source`, `schema`, `dialect`, `config`) is JSON, deflated with
  `CompressionStream`, in base64url after `#`. It is written on every edit and restored on
  load and on `hashchange`. Above 8000 characters the page drops the hash and says so.

## Stage 2

- Each diagnostic of `analyze` carries its `fix` edits, with the imports they need
  (`add_fix_imports`), so one quick fix leaves code that runs, as in `sqlty server`.
- `analyze` also returns `fixed` (`fix_source`) and `stripped` (`strip_source`): the
  **Fix all**, **Rewrite** and **Strip** buttons replace the source with them as one undo step.
- `# sqlty: ignore` comments are read by the Python half of `sqlty check`, so the page reports
  and fixes what they would silence.

## Stage 3

- `scripts/generate_playground_examples.py`, run by `make playground`, writes the numbered
  projects of `examples/` to `docs/assets/playground/examples.json` (not committed): title,
  `queries.py`, `schema.sql`, and the `dialect`, `import-style` and `rules` of `pyproject.toml`.
- The **Example** select loads one; **Tour** is the built-in sample. A docs page links to an
  example with a share link.
- `tests/playground.test.mjs` checks every example on the WebAssembly build: no `error`
  diagnostic, and nothing left to rewrite, as the `examples` hook asserts with `sqlty check`.

## Stage 4

- Each column of `analyze` names its `type_rule` and `nullability_rule` (`explain_select`),
  shown under the column in Types as `sqlty check --explain` prints them.
- Each diagnostic has its `line`; a read-only panel shows the problems as
  `sqlty check --output-format json` prints them, with lines and columns from the editor.

## `param-types`

- `add_param_types` moved from `_param_types.py` to `sqlty_python::params`, so the page's
  `sqlty.toml` takes `param-types` and **Rewrite** and **Fix all** write `SQL[R, P]`. `strip`
  drops the generated classes in Rust too.
- `examples/06_param_types/` sets `param-types = true`; the examples loader passes it on.

## Stage 6

- `scripts/check_playground.py` loads the page in Chromium: the tour gets its diagnostics, a
  share link restores a session, and **Fix all** fixes it. `docs.yml` runs it on the build,
  served under `/sqlty/` as GitHub Pages serves it, and on the deployed page after
  `deploy-pages`; `make check-playground` runs the first.
- The `wasm` profile (`opt-level = "z"`) makes the engine 6.6 MB, 1.5 MB gzipped, from 10.7 and
  2.3 MB, for about 1.6 times the analysis time (17 ms for a 600-line file). `wasm-opt` is not
  used: it would add binaryen to the build. The build job reports the size in its summary.
