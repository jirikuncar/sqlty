# VS Code for the Web

> **Status:** stage 1 merged in [#1014](https://github.com/jirikuncar/sqlty/pull/1014); stage 2 in review.
> [#1010](https://github.com/jirikuncar/sqlty/issues/1010) settled its questions: the `.vsix` ships the
> wasm, and the configuration is found as `sqlty check` finds it, `extend` and monorepos included.

Tracks [#1010](https://github.com/jirikuncar/sqlty/issues/1010): the extension does not load on
vscode.dev or github.dev, so strings marked `--sql` stay plain Python strings there.

## Stage 1: a `browser` entry

`package.json` gets `"browser": "./dist/web.js"`. `src/web.ts` only opens the output channel: the
grammars are declarative, so loading the extension is enough to highlight. `npm run test-web`
opens `fixture/` in VS Code for the Web (`@vscode/test-web`, headless Chromium) and checks that the
extension loads and activates. `_workbench.captureSyntaxTokens` is desktop only, so the desktop
smoke test keeps checking the tokens.

## Stage 2: the engine in the web extension host

Diagnostics, quick fixes, completion and hover from `sqlty_wasm.wasm`, as the docs playground runs it:

- The web extension host is already a worker, so `src/web.ts` loads the engine and calls
  `analyze`, `complete` and `hover` of `sqlty_wasm::run` from VS Code's provider API, without a
  language server. `analyze` skips the checks that read row types from Python and the
  driver-call checks (`param-count-mismatch`), as in the playground.
- `src/web/config.ts` finds the configuration as `sqlty check` does, through
  `vscode.workspace.fs`: the closest `sqlty.toml`, `pyproject.toml` or `Cargo.toml` with a sqlty
  table above the file, `extend` applied, `schema` relative to the file that set it (file,
  directory or glob). The new `config` op of `sqlty_wasm::run` reads and validates each file with
  the engine's loader, and `settings` passes the merged table, whose `types` now apply too.
- `databases`, `overrides`, `functions` and `queries` are not read on the web.
- The `.wasm` ships in the `.vsix` (`npm run build-wasm`, the cargo command of `make playground`):
  12 MB, 2.7 MB gzipped, accepted in #1010.
