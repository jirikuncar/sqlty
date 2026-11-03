# Editor integrations

> **Status:** stages 1 ([#840](https://github.com/jirikuncar/sqlty/pull/840), semantic tokens), 2
> ([#841](https://github.com/jirikuncar/sqlty/pull/841), the VS Code grammar) and 3
> ([#843](https://github.com/jirikuncar/sqlty/pull/843), the Neovim, Vim and Helix recipe) done;
> stage 4 dropped; stage 5 (Emacs, [#849](https://github.com/jirikuncar/sqlty/pull/849)) done. An
> nvim-lint linter definition is on the recipe page ([#845](https://github.com/jirikuncar/sqlty/issues/845));
> its upstream pull request has to be written by hand. Left: nvim-lspconfig and Mason
> ([#846](https://github.com/jirikuncar/sqlty/issues/846)).

Tracks [#839](https://github.com/jirikuncar/sqlty/issues/839): highlight SQL in exactly the
strings `sqlty check` checks, in VS Code and Neovim, and document the Vim and Neovim
clients.

## Decision: semantic tokens from the server, a grammar for VS Code

A TextMate grammar or a tree-sitter injection sees the source, not the value: in
`"--sql\nSELECT 1"` the `\n` is two characters, so an injected SQL grammar reads the
whole string as one `--` comment. It cannot resolve an annotation or a helper signature
either. The server already knows which strings it checks and decodes their escapes
(`locate::Queries`), so it sends the SQL tokens of exactly those strings, for every LSP
client.

VS Code shows the tokens of one semantic-token provider per document: sqlty's would
replace Pylance's for the whole file. So the VS Code extension turns highlighting off and
needs a TextMate injection instead, which can only approximate the routes.

## Stages

| Stage | Content | State |
|-------|---------|-------|
| 1 | `textDocument/semanticTokens/full` in `sqlty server`, `core.highlight_source`, `# highlighted:` mdtests | done, [#840](https://github.com/jirikuncar/sqlty/pull/840) |
| 2 | VS Code: TextMate injection for marked strings, with a fixture of strings that must and must not be highlighted | done, [#841](https://github.com/jirikuncar/sqlty/pull/841) |
| 3 | Neovim, Vim and Helix recipe page (`vim.lsp.config`, vim-lsp, ALE, coc.nvim, Helix, a quickfix list), split out of `docs/recipes/vscode.md`; skill and agents table updated | done, [#843](https://github.com/jirikuncar/sqlty/pull/843) |
| 4 | ALE linter definition around `sqlty check --output-format json` | dropped: ALE runs `sqlty server` as an LSP linter (stage 3), which also reads unsaved buffers |
| 5 | Emacs: Eglot (alone, and next to ty and ruff through `rass`) and lsp-mode (an add-on client) on the recipe page | done, [#849](https://github.com/jirikuncar/sqlty/pull/849) |

Not here: an nvim-lint linter upstream (nvim-lint takes no contributions made with generative AI;
the recipe page documents the definition users paste meanwhile, checked with Neovim 0.9),
and the nvim-lspconfig and Mason entries, which wait for their popularity thresholds.

## Stage 1

- `sqlty_sql::highlight` tokenizes a query with sqlparser. A word is a keyword only when
  it is a PostgreSQL reserved word or a clause: sqlparser knows `name`, `status` and
  `type` as keywords too. Quoted words and words after `.` are names. `%s` and
  `%(name)s` come from `placeholders`, which sqlparser reads as `%`.
- `Queries::highlights` maps the tokens of each checked string back to the source,
  split at literal boundaries and escape sequences; the marker is a comment in every
  dialect.
- The server reads helper strings from the last check and asks for
  `workspace/semanticTokens/refresh` when a check changes them.
  `initializationOptions.highlighting.enable` turns it off.

## Stage 2

- `editors/vscode/syntaxes/`: two injections into Python strings, one for single-line
  strings and one for triple-quoted ones, each split by quote character so a comment or a
  SQL string never runs past the closing quote. They begin at a `--sql` or `-- sql` marker
  right after the opening quote, and skip raw, bytes and f-strings.
- The keyword list is `sqlty_sql::highlight::KEYWORDS`; a Rust test keeps them equal.
- Not covered: helper and configured-function strings, `SQL[T]("SELECT ...")` without a
  marker, and the literals after the first of an adjacent group. A grammar cannot see
  them; VS Code shows one semantic-token provider per file, so the server cannot either.
- `fixture/highlight.py` lists strings that must and must not be highlighted; the
  extension test reads its scopes with `_workbench.captureSyntaxTokens`.

## Stage 3

- `docs/recipes/editors.md`: Neovim 0.11 (`vim.lsp.config`) and older (`vim.lsp.start`),
  the server's options, highlighting with an mdtest of which strings are colored, vim-lsp,
  ALE as an LSP linter, coc.nvim, `makeprg` for a quickfix list, and Helix.
- Checked by hand: Neovim 0.9 (`vim.lsp.start`: diagnostics and semantic tokens), Vim 9
  with ALE (diagnostics) and the `makeprg` quickfix list. vim-lsp, coc.nvim and Helix
  follow their documentation.
- A tree-sitter injection is not shipped: it reads `\n` as two characters, so it would read
  `"--sql\nSELECT 1"` as one SQL comment; semantic tokens read the value.

## Stage 5

- Checked in Docker (Debian trixie, Emacs 30.1 with Eglot 1.17 and 1.26 from GNU ELPA,
  lsp-mode from MELPA; Emacs 31.1 read for its defaults): diagnostics, the `breaking`
  baseline, the quick fix, completion and semantic tokens.
- Eglot runs one server per major mode. `rass` (rassumfrassum) multiplexes them, and Emacs 31
  offers `rass python` first. It forwards semantic-token requests to the first server but
  may take the legend of another, so sqlty goes last with highlighting off.
- lsp-mode with `lsp-semantic-tokens-enable` asks a server for tokens before `initialized`.
  `lsp_server::Connection::initialize_finish` fails on that and ruff exits; `sqlty server`
  now sends the initialize response itself and answers such a request in its main loop.
- lsp-mode colors one server's semantic tokens per buffer, as VS Code does.
