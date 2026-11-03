# Neovim, Vim, Helix and Emacs

`sqlty server` is a language server on standard input and output. Any LSP client runs it
for Python files and gets what `sqlty check` reports as you type, the fix of
`sqlty check --fix` as a quick fix, completion of table and column names, hover, and SQL
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

### nvim-lint

[nvim-lint](https://codeberg.org/mfussenegger/nvim-lint) has no sqlty linter yet. Define one
around `sqlty check --output-format json`:

```lua
local lint = require("lint")
local severities = {
  error = vim.diagnostic.severity.ERROR,
  warn = vim.diagnostic.severity.WARN,
}

lint.linters.sqlty = {
  cmd = "sqlty",
  args = { "check", "--output-format", "json" },
  stdin = false,
  ignore_exitcode = true,
  parser = function(output)
    local ok, found = pcall(vim.json.decode, output)
    if not ok or type(found) ~= "table" then
      return {}
    end
    local diagnostics = {}
    for _, d in ipairs(found) do
      table.insert(diagnostics, {
        lnum = d.start.line - 1,
        col = d.start.column - 1,
        end_lnum = d["end"].line - 1,
        end_col = d["end"].column - 1,
        severity = severities[d.severity],
        message = d.message,
        code = d.rule,
        source = "sqlty",
      })
    end
    return diagnostics
  end,
}
lint.linters_by_ft.python = { "sqlty" }

vim.api.nvim_create_autocmd("BufWritePost", {
  callback = function()
    lint.try_lint()
  end,
})
```

`sqlty check` reads the file from disk, so the linter runs on save; `sqlty server` also
checks unsaved buffers. Lines and columns in the JSON are 1-based, and nvim-lint counts
from 0.

## Settings

Every client passes the same `initializationOptions` (`init_options` in Neovim,
`:initializationOptions` in Emacs). All are optional:

```lua
init_options = {
  breaking = { against = "main", direction = "both" },
  completion = { enable = true },
  hover = { enable = true },
  highlighting = { enable = true },
}
```

| Option | Default | Meaning |
|--------|---------|---------|
| `breaking.against` | none | A baseline for `sqlty breaking`: a git revision, a `--save` snapshot or schema files |
| `breaking.direction` | `both` | `both`, `forward` or `backward` |
| `completion.enable` | `true` | Complete table and column names in query strings |
| `hover.enable` | `true` | Say what a word of a query string is: a column's type, a table's columns, a result column's inference rule |
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

## Emacs

### Eglot

Eglot is part of Emacs 29 and newer. It runs one server per major mode, so this runs
sqlty alone:

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '((python-mode python-ts-mode)
                 . ("sqlty" "server"
                    :initializationOptions (:breaking (:against "main"))))))
```

`M-x eglot` in a Python buffer starts it, or add `eglot-ensure` to `python-mode-hook`.

To run sqlty next to ty and ruff, let [rassumfrassum](https://github.com/joaotavora/rassumfrassum)
(`rass`) start them behind one connection. Emacs 31 already offers `rass python` for Python
buffers; add sqlty to it:

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '((python-mode python-ts-mode)
                 . ("rass" "python" "--" "sqlty" "server"
                    :initializationOptions
                    (:rass (:sqlty (:breaking (:against "main")
                                    :highlighting (:enable :json-false))))))))
```

Install `rass` with `uv tool install rassumfrassum` or in the project's environment. The
`rass` key of `initializationOptions` sends options to the servers whose executable matches
the key, here `sqlty` only.

!!! warning "One server colors the buffer"
    `rass` takes the semantic tokens of the first server only, with the legend of any of
    them. Keep sqlty last and its highlighting off, or its token types color ty's tokens
    wrong.

Eglot 1.20 and newer color the SQL of checked strings by themselves, with the
`eglot-semantic-keyword`, `eglot-semantic-string`, ... faces. Emacs 31 ships it; on Emacs 29
and 30, `M-x package-install RET eglot` updates it from GNU ELPA.

### lsp-mode

[lsp-mode](https://emacs-lsp.github.io/lsp-mode/) runs several servers in a buffer: register
sqlty as an add-on, and it runs next to ty, pyright or ruff:

```elisp
(with-eval-after-load 'lsp-mode
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection '("sqlty" "server"))
    :activation-fn (lsp-activate-on "python")
    :server-id 'sqlty
    :add-on? t
    :initialization-options '(:breaking (:against "main")))))
```

lsp-mode colors one server's semantic tokens per buffer, and does so only with
`lsp-semantic-tokens-enable` set. With sqlty alone in the buffer, that colors the SQL with
the `lsp-face-semhl-keyword`, ... faces; next to ty, ty's tokens win.

!!! warning "lsp-mode can stop ruff server"
    With `lsp-semantic-tokens-enable`, lsp-mode asks a server for tokens before it has
    finished starting. ruff exits on it; sqlty answers with an error and keeps running.
