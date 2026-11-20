# sqlty in a browser editor

A [CodeMirror](https://codemirror.net) editor that checks the queries of a Python
module against a schema with [`@sqlty/wasm`](..): diagnostics with quick fixes,
completion, hover and the row type of each query, all in the page.

```console
make wasm            # from the repository root: builds npm/wasm/sqlty_wasm.wasm
cd npm/wasm/example
npm install
npm run dev
```

It installs the package from `..`. In your own app, install `@sqlty/wasm` from npm.
`main.js` is the whole integration.
