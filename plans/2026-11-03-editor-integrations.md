# Editor integrations

> **Status:** stage 1 in review: `sqlty server` sends semantic tokens.

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
| 1 | `textDocument/semanticTokens/full` in `sqlty server`, `core.highlight_source`, `# highlighted:` mdtests | in review |
| 2 | VS Code: TextMate injection for marked and `SQL[T](...)` strings, with a fixture of strings that must and must not be highlighted | todo |
| 3 | Vim and Neovim recipe page (`vim.lsp.config`, vim-lsp, ALE, coc.nvim, Helix), split out of `docs/recipes/vscode.md`; skill and agents table updated | todo |
| 4 | ALE linter definition around `sqlty check --output-format json` | todo |

Not here: an nvim-lint linter (nvim-lint takes no contributions made with generative AI),
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
