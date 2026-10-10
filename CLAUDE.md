# CLAUDE.md

See [AGENTS.md](AGENTS.md) for full development conventions.

## Quick reference

```bash
uv sync --all-groups          # install
uv run pytest                 # all tests
make test-mdtest              # mdtest only
make update-mdtest            # update mdtest assertions
make typecheck                # mypy + ty
uv run pre-commit run --all-files  # lint + format (run before every commit)
```

## Key conventions

- **Never force-push.** No `--force`, no `--force-with-lease`, no exceptions.
- **Always run `uv run pre-commit run --all-files` before committing.**
  This runs mypy, ruff (lint + format), and other checks. Fix all errors
  before creating a commit.
- **Use `--sql\n`** (newline escape), not `--sql ` (space).
  `--sql SELECT ...` with a space is a SQL comment — the parser sees an
  empty query and the string is not wrapped. The rewriter never modifies
  string content — it only adds/removes the `SQL[T]()` wrapper,
  guaranteeing `strip(rewrite(x)) == x`.
- **Every new feature gets an mdtest** in `tests/mdtest/`. Use `make update-mdtest`
  to fill expected values.
- **README.md is tested** — it is an mdtest file, run with `tests/mdtest/`.
- `SQL[T]` is a `str` subclass. It works anywhere a string works.
- Docstrings and `--help` text should explain usage for both humans and AI agents.
