---
name: sqlty-server
description: Run sqlty in an editor with sqlty server, the language server, and the VS Code extension. Use when setting up sqlty in VS Code, Cursor, VSCodium, Neovim, Vim, Helix, Emacs or another LSP client, highlighting SQL in query strings, fixing a query on save with source.fixAll.sqlty, showing sqlty breaking findings while editing, or when the editor reports nothing or different diagnostics than sqlty check.
---

# sqlty server

`sqlty server` is a language server on standard input and output. It reports what
`sqlty check` reports while you type, each diagnostic under the name, row type or
query it is about, with the fix of `sqlty check --fix` as a quick fix. In a query
string it completes table and column names, and hover shows a column's type, a table's
columns or a result column's inference rule.

Run `sqlty version`: this skill may be newer or older than the installed sqlty.
When it and `sqlty server --help` disagree, trust the command.

```toml
[tool.sqlty]
schema = "schema.sql"
```

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
```

```python
from sqlty import SQL

# error: [unknown-column] "column `nmae` does not exist in `users`"
q = SQL[tuple[int, str]]("--sql\nSELECT id, nmae FROM users")
```

## VS Code

Install **sqlty** (`jirikuncar.sqlty`) from the Visual Studio Marketplace, or from
Open VSX in Cursor, VSCodium and other builds of VS Code:

```bash
code --install-extension jirikuncar.sqlty
```

The extension bundles no sqlty. It runs the one your project pins, so add it to the
project's environment (`uv add --dev sqlty`) and the editor reports what CI reports.

| Setting | Meaning |
|---------|---------|
| `sqlty.path` | `sqlty` executables to try first |
| `sqlty.interpreter` | The interpreter whose `sqlty` runs; empty is the one the Python extension selects |
| `sqlty.breaking.against` | A baseline for `sqlty breaking`: a git revision, a `--save` snapshot or schema files |
| `sqlty.breaking.direction` | `both` (default), `forward` or `backward` |
| `sqlty.trace.server` | `off` (default), `messages` or `verbose`: log the protocol in the **sqlty** output channel |

Fix a file on every save with `source.fixAll.sqlty`, which is `sqlty check --fix`:

```json
{
  "[python]": {
    "editor.codeActionsOnSave": {
      "source.fixAll.sqlty": "explicit"
    }
  }
}
```

## Other editors

Any LSP client can run `sqlty server` for Python files. For Neovim 0.11:

```lua
vim.lsp.config("sqlty", {
  cmd = { "sqlty", "server" },
  filetypes = { "python" },
  root_markers = { "pyproject.toml", ".git" },
  init_options = { breaking = { against = "main", direction = "both" } },
})
vim.lsp.enable("sqlty")
```

`init_options` is optional: without it, no baseline is checked. The server also
colors the SQL of checked strings with semantic tokens; `highlighting = { enable = false }`
turns that off. VS Code shows one semantic-token provider per file, so the extension
keeps it off there and highlights strings marked `--sql` with a grammar instead.

For nvim-lint, Vim (vim-lsp, ALE, coc.nvim, `:make`), Helix and Emacs (Eglot, lsp-mode), see
[Neovim, Vim, Helix and Emacs](https://jirikuncar.github.io/sqlty/recipes/editors/).
Eglot runs one server per major mode: run sqlty next to ty or ruff through
`rass python -- sqlty server`, with sqlty's highlighting off, or use lsp-mode with
sqlty as an add-on.

## Breaking changes while editing

With a baseline, the editor also shows what `sqlty breaking` reports. A finding's
source is `sqlty breaking (forward)` or `sqlty breaking (backward)`, and it has no
quick fix: the fix belongs to the migration or the code, depending on which ships
first (see `sqlty-breaking`). The server resolves `main` when the project loads:
when it moves, save a schema or run **sqlty: Restart Server**.

## Nothing is reported

- Run `sqlty check <file>` in a terminal. When it reports the problem and the
  editor doesn't, the editor runs another `sqlty`: check `sqlty.path` and
  `sqlty.interpreter`, then run **sqlty: Restart Server**.
- Saving `pyproject.toml` or a `.sql` file reloads the settings and schema
  without a restart.
- The extension does not run in Restricted Mode: trust the folder.

## Reference

- [VS Code](https://jirikuncar.github.io/sqlty/recipes/vscode/)
- [Neovim, Vim, Helix and Emacs](https://jirikuncar.github.io/sqlty/recipes/editors/)
- [Breaking schema changes](https://jirikuncar.github.io/sqlty/guide/breaking-changes/)
