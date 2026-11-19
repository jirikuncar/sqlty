<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/sqlty-wordmark-dark.svg">
    <img src="https://raw.githubusercontent.com/jirikuncar/sqlty/main/docs/assets/sqlty-wordmark-light.svg" alt="SQLTy" width="300">
  </picture>
</h1>

The sqlty engine compiled to WebAssembly: type-check, complete and fix the SQL in
Python source from a browser, an editor or Node.js. The
[playground](https://sqlty.kuncar.dev/playground/) and the VS Code extension for the
web run it.

```console
npm install @sqlty/wasm
```

```js
import { load } from "@sqlty/wasm";

const sqlty = await load();
const answer = sqlty({
  op: "analyze",
  source: 'q = "--sql\\nSELECT id, name FROM users"',
  schema: "CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL);",
});
answer.queries[0].row_type; // "tuple[int, str]"
```

`load()` fetches `sqlty_wasm.wasm` next to the module, or reads it in Node.js. Pass
its URL, a `Response` or its bytes when a bundler moves it, such as
`load(wasm)` after `import wasm from "@sqlty/wasm/sqlty_wasm.wasm?url"` in Vite.
`@sqlty/wasm/engine` takes the bytes only, with no fetching or file access.
`sqlty_wasm_core.wasm` leaves out sqruff, about a third smaller to download: it
answers `format`, and a check with the `sqruff` rule on, with `needs: "sqruff"`.

Requests are `analyze` (diagnostics with their quick fixes, row types, and what
`sqlty rewrite`, `strip` and `check --fix` make of the source), `complete`,
`hover`, `format`, `config` and `project`. Offsets count UTF-16 code units, as
JavaScript strings do. `index.d.ts` types each one.

The engine checks one module at a time and runs no Python: a helper or a type
imported from another module is not followed. For a whole project, run the
[`sqlty` CLI](https://sqlty.kuncar.dev/installation/).

Documentation: <https://sqlty.kuncar.dev/recipes/embedding/>
