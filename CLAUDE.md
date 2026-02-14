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

- **Prefer `--sql\n`** (newline escape) over `--sql ` (space) in new code.
  Both are accepted. The rewriter never modifies string content — it only
  adds/removes the `SQL[T]()` wrapper, guaranteeing `strip(rewrite(x)) == x`.
- **Every new feature gets an mdtest** in `tests/mdtest/`. Use `make update-mdtest`
  to fill expected values.
- **README.md is tested** — code blocks with assertion comments are executed
  via `tests/test_readme.py`.
- `SQL[T]` is a `str` subclass. It works anywhere a string works.
- Docstrings and `--help` text should explain usage for both humans and AI agents.
