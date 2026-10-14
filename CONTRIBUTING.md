# Contributing to SQLTy

Thanks for your interest. Bug reports, feature requests, docs fixes and code are
all welcome. By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Search [existing issues](https://github.com/jirikuncar/sqlty/issues) first.
- Open an issue before a large change, so the design can be agreed on before
  you write code.
- Report security vulnerabilities privately, see [SECURITY.md](SECURITY.md).

## Development

Setup, tests and architecture: [DEVELOPMENT.md](DEVELOPMENT.md).
Conventions for both humans and AI agents: [AGENTS.md](AGENTS.md).

```bash
uv sync --all-groups
uv run pre-commit install
uv run pytest
```

## Pull requests

1. Fork the repository and create a branch from `main`.
2. Add tests. Every new feature or behavior change needs an mdtest in
   `tests/mdtest/`; user-visible behavior also belongs in `docs/`.
   CLI changes need an e2e test in `tests/e2e/`.
3. Run `uv run pre-commit run --all-files` and `uv run pytest`.
4. Keep the pull request focused on one change and describe what changed and why.
   Reference the issue it resolves (`Fixes #123`).
5. Fill in the pull request template, including the AI disclosure.

Reviewers apply [REVIEW.md](REVIEW.md). CI must pass before merge.

## AI-assisted contributions

SQLTy is developed with AI coding agents, and contributions made with them are
welcome under these rules:

- **Disclose it.** State in the pull request description which tool was used and
  for what (code, tests, docs, review). Add a `Co-Authored-By:` trailer to commits
  written with an agent. Autocomplete of single lines needs no disclosure.
- **You are the author.** You must understand, run and be able to explain every
  line you submit. Answer review comments yourself.
- **No unreviewed output.** Pull requests, issues and comments that are visibly
  unedited agent output may be closed without review.
- **Verify claims.** Behavior claims in descriptions and docs must be backed by
  tests or references, not by the agent's say-so.

## Releases

Maintainers publish releases from GitHub Releases; the
[Release workflow](.github/workflows/pypi.yml) builds wheels and uploads them to
PyPI with trusted publishing. The version comes from `rust/Cargo.toml`.

## License

Contributions are licensed under the [MIT License](LICENSE).
