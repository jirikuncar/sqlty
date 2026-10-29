# sqlty for VS Code

Type-check the SQL in your Python code as you type: the diagnostics of `sqlty check`, each
on the span it is about, with a quick fix where `sqlty check --fix` has one.

The extension runs `sqlty server` from your project, so the editor and CI agree. Add sqlty
to the project's environment (`uv add --dev sqlty`); the extension bundles none.

The [documentation](https://jirikuncar.github.io/sqlty/recipes/vscode/) covers the setup,
Restricted Mode and other editors.

## Settings

| Setting | Default | Meaning |
|---------|---------|---------|
| `sqlty.path` | `[]` | `sqlty` executables to try first |
| `sqlty.interpreter` | `[]` | The interpreter whose `sqlty` runs; empty is the one selected in the Python extension |
| `sqlty.breaking.against` | empty | A baseline for `sqlty breaking`: a git revision, a `--save` snapshot or schema files |
| `sqlty.breaking.direction` | `both` | `both`, `forward` or `backward`, as `sqlty breaking --direction` |
| `sqlty.trace.server` | `off` | Log the protocol messages in the output channel |

`sqlty` is looked up in `sqlty.path`, then in the interpreter's environment, then on `PATH`.
A `sqlty` without `sqlty server` is skipped.

## Breaking changes

With `sqlty.breaking.against` set to `main`, each check also runs `sqlty breaking --against main`:

- Forward, with every check: your buffer's queries against the schema of `main`. Source:
  `sqlty breaking (forward)`.
- Backward, when the project loads and when a schema or `pyproject.toml` is saved: the
  queries of `main` against your schema, in every file. Source: `sqlty breaking (backward)`.
  A finding in a file you have changed since sits on its first line and names its line in
  `main`.

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

`npm run package` writes `sqlty.vsix`. Each GitHub release publishes it under the
release's version, to the Visual Studio Marketplace (`VSCE_PAT`) and Open VSX (`OVSX_PAT`),
secrets of the `vscode` environment (`.github/workflows/vscode.yml`).
