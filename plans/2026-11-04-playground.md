# Playground: share links, configuration, fixes and breaking changes

> **Status:** stage 1 (share links and configuration) in review in [#895](https://github.com/jirikuncar/sqlty/pull/895).
> Stages 2 to 6 not started.

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
