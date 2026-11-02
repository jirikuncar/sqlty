# Neovim, Vim and Helix

`sqlty server` is a language server on standard input and output. Any LSP client runs it
for Python files and gets what `sqlty check` reports as you type, the fix of
`sqlty check --fix` as a quick fix, completion of table and column names, and SQL
highlighting in the strings `sqlty check` checks.

Add sqlty to the project's environment (`uv add --dev sqlty`) and start the editor from
it, or point the client at `.venv/bin/sqlty`, so the editor runs the version CI runs.

## Neovim

Neovim 0.11 and newer:

```lua
vim.lsp.config("sqlty", {
  cmd = { "sqlty", "server" },
  filetypes = { "python" },
  root_markers = { "pyproject.toml", ".git" },
})
vim.lsp.enable("sqlty")
```

Neovim 0.10 and older start the client when a Python buffer opens:

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = "python",
  callback = function(args)
    vim.lsp.start({
      name = "sqlty",
      cmd = { "sqlty", "server" },
      root_dir = vim.fs.root(args.buf, { "pyproject.toml", ".git" }),
    })
  end,
})
```

### Highlighting

The server sends the SQL of each checked string as semantic tokens: keywords, strings,
numbers, comments, operators and placeholders. Neovim colors them with its default
`@lsp.type.keyword`, `@lsp.type.string`, ... highlight groups, so a theme needs no change.

```python
from typing import Annotated, LiteralString
from sqlty import SQL

def fetch_all[T](query: Annotated[LiteralString, SQL], form: type[T]) -> list[T]: ...

q = "--sql\nSELECT name FROM users WHERE id = $1"
fetch_all("SELECT id FROM users", tuple[int])
plain = "SELECT name FROM users"
# highlighted: comment --sql
# highlighted: keyword SELECT
# highlighted: keyword FROM
# highlighted: keyword WHERE
# highlighted: operator =
# highlighted: parameter $1
# highlighted: keyword SELECT
# highlighted: keyword FROM
```

The helper's query is colored because `sqlty check` checks it; `plain` stays a Python
string. Names are left uncolored: only reserved words and clauses are keywords, so a
column named `name` or `status` reads as a name.

!!! note "Semantic tokens need no tree-sitter query"
    A tree-sitter injection reads the source, where `\n` is two characters, so it would
    read `"--sql\nSELECT 1"` as one SQL comment. The server reads the string's value.

## Settings

Every client passes the same `initializationOptions` (`init_options` in Neovim). All are
optional:

```lua
init_options = {
  breaking = { against = "main", direction = "both" },
  completion = { enable = true },
  highlighting = { enable = true },
}
```

| Option | Default | Meaning |
|--------|---------|---------|
| `breaking.against` | none | A baseline for `sqlty breaking`: a git revision, a `--save` snapshot or schema files |
| `breaking.direction` | `both` | `both`, `forward` or `backward` |
| `completion.enable` | `true` | Complete table and column names in query strings |
| `highlighting.enable` | `true` | Send the SQL of checked strings as semantic tokens |

## Vim

With [vim-lsp](https://github.com/prabirshrestha/vim-lsp):

```vim
if executable('sqlty')
  au User lsp_setup call lsp#register_server({
      \ 'name': 'sqlty',
      \ 'cmd': {server_info->['sqlty', 'server']},
      \ 'allowlist': ['python'],
      \ })
endif
```

With [ALE](https://github.com/dense-analysis/ale), as a language server:

```vim
call ale#linter#Define('python', {
\   'name': 'sqlty',
\   'lsp': 'stdio',
\   'executable': 'sqlty',
\   'command': '%e server',
\   'project_root': function('ale#python#FindProjectRoot'),
\})
```

With [coc.nvim](https://github.com/neoclide/coc.nvim), in `coc-settings.json`:

```json
{
  "languageserver": {
    "sqlty": {
      "command": "sqlty",
      "args": ["server"],
      "filetypes": ["python"],
      "rootPatterns": ["pyproject.toml", ".git"]
    }
  }
}
```

Without a language server client, `:make` runs `sqlty check` on the project and
`:copen` lists its diagnostics:

```vim
setlocal makeprg=sqlty\ check
setlocal errorformat=%f:%l:\ %m,%-G%.%#
```

## Helix

In `languages.toml`, add sqlty to the servers of Python. Helix replaces the default list
with yours, so list the servers you use too:

```toml ignore
[language-server.sqlty]
command = "sqlty"
args = ["server"]

[[language]]
name = "python"
language-servers = ["ty", "ruff", "sqlty"]
```

Helix shows the diagnostics, quick fixes and completion. It does not ask for semantic
tokens, so query strings stay Python strings.
