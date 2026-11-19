---
description: Embed the sqlty engine in a browser app, an editor or a Node.js tool with the @sqlty/wasm npm package, compiled to WebAssembly.
---

# Embedding in JavaScript

[`@sqlty/wasm`](https://www.npmjs.com/package/@sqlty/wasm) is the sqlty engine
compiled to WebAssembly, the one the [playground](../playground.md) and
[VS Code for the Web](vscode.md#vs-code-for-the-web) run. It checks the queries of a
Python module against a schema in a browser, an editor extension or Node.js 20.16 or
newer, with no server and no Python.

```console
npm install @sqlty/wasm
```

## Check a module

`load()` starts the engine and answers JSON requests synchronously:

```js
import { load } from "@sqlty/wasm";

const sqlty = await load();
const answer = sqlty({
  op: "analyze",
  source: 'q = "--sql\\nSELECT id, name FROM users"',
  schema: "CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL);",
});
answer.queries[0].row_type; // "tuple[int, str]"
answer.diagnostics; // what `sqlty check` reports, each with its quick fix
```

A request takes the module as `source`, the `CREATE TABLE` statements as `schema`, a
`dialect` (`postgres` by default) and a `sqlty.toml` as `config`. The answer is `{"error":
...}` when a request is invalid.

| `op` | Also takes | Answers |
| --- | --- | --- |
| `analyze` | | `diagnostics`, `queries` (row type, columns and parameters), `highlights`, and the source as `sqlty rewrite`, `strip` and `check --fix` leave it: `rewritten`, `stripped`, `fixed` |
| `complete` | `offset` | The tables and columns to complete at `offset`, and the range they replace; `null` outside a query |
| `hover` | `offset` | What the word at `offset` is, in Markdown; `null` for nothing |
| `format` | | `formatted`, as `sqlty format` leaves the source |
| `config` | `path` | The validated sqlty table of the file `path` holding `source` |
| `project` | `path`, `files` | The configuration `sqlty check` finds for the directory `path`, asking for each file it reads |

Offsets and ranges count UTF-16 code units, as JavaScript strings and most editors
do. `index.d.ts` types each request and answer.

## Load the module

`load()` fetches `sqlty_wasm.wasm` next to the package's module in a browser and
reads it from disk in Node.js. When a bundler moves the file, pass its URL, a
`Response` or its bytes:

```js
import { load } from "@sqlty/wasm";
import wasm from "@sqlty/wasm/sqlty_wasm.wasm?url"; // Vite

const sqlty = await load(wasm);
```

`sqlty_wasm_core.wasm` is the engine without sqruff, about a third smaller to
download. It answers `format`, and an `analyze` or `breaking` with the
[`sqruff`](../reference/rules/sqruff.md) rule on, with `{"error": ..., "needs": "sqruff"}`:
load `sqlty_wasm.wasm` then, as the playground does on the first such request.

```js
import core from "@sqlty/wasm/sqlty_wasm_core.wasm?url"; // Vite

const sqlty = await load(core);
```

`@sqlty/wasm/engine` starts the engine from bytes alone, with no fetching or file
access, as the VS Code extension does. A check runs on the calling thread: run the
engine in a Web Worker to keep a page responsive on large modules.

## Example: a browser editor

[`npm/wasm/example`](https://github.com/jirikuncar/sqlty/tree/main/npm/wasm/example)
is a CodeMirror editor that shows sqlty's diagnostics with their quick fixes,
completes table and column names, and shows the row type of each query, in about 100
lines of JavaScript.

## Limits

The engine checks one module at a time and runs no Python: a helper or a type
imported from another module is not followed, and keys of `sqlty.toml` that name files,
such as `schema`, are errors in `config`. The `project` request reads a project's
configuration through the files the host passes it. For a whole project, run the
[`sqlty` CLI](../installation.md).
