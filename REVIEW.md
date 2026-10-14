# Review instructions

## What Important means here

Reserve Important for findings that break behavior users rely on:

- Wrong inferred types, or `sqlty check` missing or inventing a diagnostic.
- `rewrite`/`strip` changing string content, or breaking
  `strip(rewrite(x)) == x` (see the import-line exception in AGENTS.md).
- Non-idempotent `rewrite`, `strip` or `check --fix`.
- Rust engine panics reachable from Python input.
- The magic codec or `sqlty_magic.pth` affecting files without `# coding: sqlty`,
  or failing to return the source unchanged on error.
- `src/sqlty/_core.pyi` out of sync with `rust/src/lib.rs`.
- Wheel or sdist missing `sqlty_magic.pth`.

Style, naming and refactoring suggestions are Nit at most.

## Always check

- New features and behavior changes have an mdtest in `tests/mdtest/`;
  user-visible behavior is also documented in `docs/`.
- New lint rules have a page in `src/sqlty/rules/` and a row in
  `docs/reference/rules.md`.
- Test SQL uses `--sql\n`, never `--sql ` followed by a space.
- Docstrings and `--help` text explain usage, with a copy-paste example.
- Pull requests made with an AI agent disclose it, as required by
  CONTRIBUTING.md.

## Do not report

- Anything CI enforces: ruff, mypy, ty, rustfmt, clippy.
- `uv.lock`, `rust/Cargo.lock` and expected values filled in by
  `make update-mdtest` or `--inline-snapshot=update`, unless they contradict the change.
- `PLAN-FUTURE.md`.

## Cap the nits

Report at most five Nits per review and count the rest in the summary. After the
first review of a pull request, report Important findings only.
