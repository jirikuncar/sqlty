-- Neovim for the recording: no plugins; sqlty server and ty server for diagnostics and quick fixes.
vim.o.number = true
vim.o.autoindent = false
vim.o.smartindent = false
vim.o.expandtab = true
vim.o.shiftwidth = 4
vim.o.swapfile = false
vim.o.autoread = true
vim.o.hlsearch = false
vim.o.linebreak = true
vim.o.signcolumn = "yes"
vim.o.laststatus = 2
vim.o.shortmess = vim.o.shortmess .. "IF"
vim.cmd("filetype on | filetype plugin off | filetype indent off | syntax on")

vim.diagnostic.config({ virtual_text = { prefix = "x" }, underline = true, signs = true, update_in_insert = false })

vim.lsp.config("sqlty", {
  cmd = { "uv", "run", "--quiet", "sqlty", "server" },
  filetypes = { "python" },
  root_markers = { "pyproject.toml" },
})
vim.lsp.config("ty", {
  cmd = { "uv", "run", "--quiet", "ty", "server" },
  filetypes = { "python" },
  root_markers = { "pyproject.toml" },
})
vim.lsp.enable({ "sqlty", "ty" })

-- VS Code tells the server when a migration changes on disk (a merge, a branch switch), and asks for
-- fresh diagnostics when a file comes back into view; Neovim needs telling.
vim.api.nvim_create_autocmd({ "BufEnter", "FocusGained" }, {
  callback = function(event)
    for _, client in ipairs(vim.lsp.get_clients({ name = "sqlty" })) do
      client:notify("workspace/didChangeWatchedFiles", {
        changes = { { uri = vim.uri_from_fname(client.root_dir .. "/pyproject.toml"), type = 2 } },
      })
    end
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = event.buf, method = "textDocument/diagnostic" })) do
      local params = { textDocument = vim.lsp.util.make_text_document_params(event.buf) }
      client:request("textDocument/diagnostic", params, vim.lsp.diagnostic.on_diagnostic, event.buf)
    end
  end,
})
