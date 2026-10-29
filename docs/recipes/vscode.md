# VS Code

The sqlty extension shows what `sqlty check` reports while you type, each diagnostic
under the name, row type or query it is about, with the fix of `sqlty check --fix` as a
quick fix.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```toml
[tool.sqlty]
schema = "schema.sql"
```

```python
from sqlty import SQL

# error[45:49]: [unknown-column] "column `nmae` does not exist in `users`"
q = SQL[tuple[int, str]]("--sql\nSELECT id, nmae FROM users")
```

The editor underlines `nmae`, and hovering it shows the message and a link to the
rule's page.

## Install

Install **sqlty** from the
[Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=jirikuncar.sqlty),
or from [Open VSX](https://open-vsx.org/extension/jirikuncar/sqlty) in Cursor, VSCodium and
other editors built on VS Code:

```bash
code --install-extension jirikuncar.sqlty
```

The extension runs `sqlty server` from your project's environment, so add sqlty there:

```bash
uv add --dev sqlty
```

!!! info "The editor and CI run the same sqlty"
    The extension bundles no sqlty. It runs the version your project pins, with your
    `[tool.sqlty]` settings, so it reports what `sqlty check` reports in CI.

## How the server is found

The first `sqlty` that has `sqlty server` runs:

| Order | Where | Set by |
|-------|-------|--------|
| 1 | Each path of `sqlty.path` | You |
| 2 | `python -m sqlty` of the interpreter | `sqlty.interpreter`, else the one selected in the Python extension |
| 3 | `sqlty` on `PATH` | Your shell |

The extension restarts the server when one of these changes, or when you run
**sqlty: Restart Server**. Saving `pyproject.toml` or a `.sql` file reloads the
project's settings and schema without a restart.

## Fix on save

`source.fixAll.sqlty` is `sqlty check --fix` on the file: a stale `SQL[T]` is rewritten,
an inline `--sql` comment is split onto its own line, and so on. Run it on every save:

```json
{
  "[python]": {
    "editor.codeActionsOnSave": {
      "source.fixAll.sqlty": "explicit"
    }
  }
}
```

## Breaking changes

Set `sqlty.breaking.against` to a baseline, and the editor also shows what
[`sqlty breaking`](../guide/breaking-changes.md) reports against it:

```json
{
  "sqlty.breaking.against": "main"
}
```

| Direction | What is checked | When |
|-----------|-----------------|------|
| `forward` | Your queries against the schema of the baseline | With every check of a file |
| `backward` | The queries of the baseline against your schema | When the project loads, and when you save a schema or `pyproject.toml` |

`sqlty.breaking.direction` picks one; the default is `both`. A finding's source is
`sqlty breaking (forward)` or `sqlty breaking (backward)`, and it has no quick fix: the
fix belongs to the migration or the code, depending on which ships first.

!!! note "A moved baseline is read again on reload"
    The server resolves `main` when the project loads. When `main` moves, save a schema
    or run **sqlty: Restart Server**.

## Settings

| Setting | Default | Meaning |
|---------|---------|---------|
| `sqlty.path` | `[]` | `sqlty` executables to try first |
| `sqlty.interpreter` | `[]` | The interpreter whose `sqlty` runs; empty is the one selected in the Python extension |
| `sqlty.breaking.against` | empty | A baseline for `sqlty breaking`: a git revision, a `--save` snapshot or schema files |
| `sqlty.breaking.direction` | `both` | `both`, `forward` or `backward` |
| `sqlty.trace.server` | `off` | Log the protocol messages in the **sqlty** output channel |

## Restricted Mode

!!! warning "The extension does not run in Restricted Mode"
    The server comes from the folder's own environment, so a folder you have not trusted
    would choose what runs. Trust the folder to turn the extension on.

## Other editors

`sqlty server` is a language server on standard input and output. Any LSP client can run
it for Python files. For Neovim 0.11:

```lua
vim.lsp.config("sqlty", {
  cmd = { "sqlty", "server" },
  filetypes = { "python" },
  root_markers = { "pyproject.toml", ".git" },
  init_options = { breaking = { against = "main", direction = "both" } },
})
vim.lsp.enable("sqlty")
```

`init_options` is optional: without it, no baseline is checked.
