-- Neovim for the recording: no plugins, sqlty server for diagnostics and quick fixes.
vim.o.number = true
vim.o.autoindent = false
vim.o.smartindent = false
vim.o.expandtab = true
vim.o.shiftwidth = 4
vim.o.swapfile = false
vim.o.autoread = true
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
vim.lsp.enable("sqlty")
