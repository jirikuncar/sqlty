# VS Code for the Web

> **Status:** stage 1 merged in [#1014](https://github.com/jirikuncar/sqlty/pull/1014); stage 2 waits on the
> `.vsix` size and config scope questions of [#1010](https://github.com/jirikuncar/sqlty/issues/1010).

Tracks [#1010](https://github.com/jirikuncar/sqlty/issues/1010): the extension does not load on
vscode.dev or github.dev, so strings marked `--sql` stay plain Python strings there.

## Stage 1: a `browser` entry

`package.json` gets `"browser": "./dist/web.js"`. `src/web.ts` only opens the output channel: the
grammars are declarative, so loading the extension is enough to highlight. `npm run test-web`
opens `fixture/` in VS Code for the Web (`@vscode/test-web`, headless Chromium) and checks that the
extension loads and activates. `_workbench.captureSyntaxTokens` is desktop only, so the desktop
smoke test keeps checking the tokens.

## Stage 2: the engine in a web worker

Diagnostics, completion and hover from `sqlty_wasm.wasm`, as the docs playground runs it:

- A language server in a web worker (`vscode-languageclient/browser`) answers `analyze`,
  `complete` and `hover` requests of `sqlty_wasm::run`. The playground's `analyze` skips the checks
  that read row types from Python and the driver-call checks (`param-count-mismatch`); the web
  extension skips the same.
- `schema.sql` and `[tool.sqlty]` are read through `vscode.workspace.fs`, since the web host has
  no file system. `sqlty_wasm::run` takes a `sqlty.toml` text; a `pyproject.toml` needs its
  `[tool.sqlty]` table passed instead.
- The `.wasm` ships in the `.vsix`, built by the cargo command of `make playground`. It is 12 MB
  uncompressed: check the `.vsix` size, since the desktop extension does not need it.
