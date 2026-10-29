# sqlty for VS Code

Type-check the SQL in your Python code as you type: the diagnostics of `sqlty check`, each
on the span it is about, with a quick fix where `sqlty check --fix` has one.

The extension runs `sqlty server` from your project, so the editor and CI agree.

## Settings

| Setting | Default | Meaning |
|---------|---------|---------|
| `sqlty.path` | `[]` | `sqlty` executables to try first |
| `sqlty.interpreter` | `[]` | The interpreter whose `sqlty` runs; empty is the one selected in the Python extension |
| `sqlty.importStrategy` | `fromEnvironment` | `fromEnvironment` or `useBundled` |
| `sqlty.trace.server` | `off` | Log the protocol messages in the output channel |

`sqlty` is looked up in `sqlty.path`, then in the interpreter's environment, then on `PATH`.
A `sqlty` without `sqlty server` is skipped.

## Fix on save

```json
{
  "[python]": {
    "editor.codeActionsOnSave": {
      "source.fixAll.sqlty": "explicit"
    }
  }
}
```

## Development

```bash
npm ci
npm run check    # tsc, oxlint, oxfmt
npm test         # opens fixture/ in VS Code; needs a sqlty with `sqlty server` on PATH
```
