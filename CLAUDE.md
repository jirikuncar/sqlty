# CLAUDE.md

See [AGENTS.md](AGENTS.md) for full development conventions.

## Quick reference

```bash
uv sync --all-groups          # install
uv run pytest                 # all tests
make test-mdtest              # mdtest only
make update-mdtest            # update mdtest assertions
make typecheck                # mypy + ty
```

## Key conventions

- **Always use `--sql\n`** (newline escape), never `--sql ` (space) in strings.
  Space after `--sql` makes the entire query a SQL comment.
  The rewriter normalizes `--sql ` → `--sql\n` but canonical form is `\n`.
- **Every new feature gets an mdtest** in `tests/mdtest/`. Use `make update-mdtest`
  to fill expected values.
- **README.md is tested** — code blocks with assertion comments are executed
  via `tests/test_readme.py`.
- `SQL[T]` is a `str` subclass. It works anywhere a string works.
- Docstrings and `--help` text should explain usage for both humans and AI agents.
