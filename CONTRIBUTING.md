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
uv run prek install
uv run pytest
```

## Pull requests

1. Fork the repository and create a branch from `main`.
2. Add tests. Every new feature or behavior change needs an mdtest in
   `tests/mdtest/`; user-visible behavior also belongs in `docs/`.
   CLI changes need an e2e test in `tests/e2e/`. A change to how an operator,
   aggregate or JSON accessor is typed is an inference rule: follow
   [`rust/crates/sqlty-sql/rules/README.md`](rust/crates/sqlty-sql/rules/README.md).
3. Run `uv run prek run --all-files` and `uv run pytest`.
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

## Lint rule defaults

A new rule ships on, so its check runs for everyone:

- `error` when it reports SQL or code that fails at runtime, such as a column the
  table does not have;
- `warn` otherwise, including a rule that reports SQL sqlty can't type: the user learns
  why a query is unchecked, and CI does not fail on sqlty's gaps.

It ships off only when it conflicts with an existing rule or a documented workflow,
and its rule page says which. `redundant-optional` and `redundant-literal` are off
because they reject the widened annotations of an expand and contract migration.

The default set changes only in a major version. Before 1.0.0, any minor version may
change it, and the release notes list each change.

## Releases

Maintainers publish releases from GitHub Releases; the
[Release workflow](.github/workflows/pypi.yml) builds wheels and uploads them to
PyPI with trusted publishing. The version comes from `rust/Cargo.toml` (`[workspace.package]`).

To release:

1. Set the version in `rust/Cargo.toml` and in the plugin manifests
   (`.claude-plugin/`, `.codex-plugin/`, `.cursor-plugin/`), and merge it.
2. Run the Release workflow on `main` by hand (`workflow_dispatch`). Pull requests
   build one wheel per OS; this run builds every wheel and the sdist, installs each wheel on every supported Python, and publishes
   to TestPyPI. Install from there and run `sqlty version`.
3. Publish a GitHub Release tagged `vX.Y.Z`. The workflow builds again and uploads to
   PyPI.

Supporting a new CPython means adding it to `requires-python` and the classifiers in
`pyproject.toml`, the test matrix in `.github/workflows/tests.yml`, and the wheel
build in `.github/workflows/pypi.yml`. Dropping one removes it from the same places.
See [Supported Python versions](docs/installation.md#supported-python-versions).

## License

Contributions are licensed under the [MIT License](LICENSE).
