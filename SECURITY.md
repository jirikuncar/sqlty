# Security Policy

## Supported versions

Only the latest release receives security fixes, in every channel it ships
through: PyPI (`sqlty`, on the two latest CPython releases, see
[Supported Python versions](https://sqlty.kuncar.dev/installation/#supported-python-versions)),
npm (`sqlty`, `@sqlty/wasm`), Homebrew, the VS Code Marketplace and Open VSX.

## Reporting a vulnerability

Do not open a public issue. Report it privately through
[GitHub private vulnerability reporting](https://github.com/jirikuncar/sqlty/security/advisories/new).

Include the affected version and channel, a minimal reproduction and the
impact you expect. [THREAT_MODEL.md](THREAT_MODEL.md) lists what is in scope
and how severity is rated. Crashes, stalls and slow inputs are bugs, not
vulnerabilities, unless they show memory corruption: open an issue (sqlty prints a link to a prefilled one).

You can expect an answer within 7 days, and a fix or a plan within 30 days of
a confirmed report. The advisory is published within 90 days, or as soon as
the fix is released. It credits the reporter unless you ask otherwise.

## How reports are handled

1. **Triage.** The report arrives as an advisory in `Triage`. Reproduce it on
   the latest release and rate it with [THREAT_MODEL.md](THREAT_MODEL.md).
   Not a vulnerability: say why in a comment and close it, or ask the reporter
   to open an issue. Otherwise: **Accept and open as draft**.
2. **Draft.** Fill in the affected packages (`pip` `sqlty`, `npm` `sqlty` and
   `@sqlty/wasm` where they apply), the affected versions and the severity.
   Request a CVE from the draft once the impact is clear; GitHub reviews it
   within about 72 hours and nothing becomes public.
3. **Fix.** Start a temporary private fork from the draft. CI does not run
   there, so run the affected tests locally (`make test`, plus `cargo test` in
   `rust/` for Rust changes) and add a regression test. The fork merges one
   pull request into `main`; put the version bump
   (`uv run python scripts/bump_version.py X.Y.Z`) in the same pull request.
   Keep the commit message neutral: it is public on merge.
4. **Release.** Merge from the advisory page, then follow the release steps in
   [CONTRIBUTING.md](CONTRIBUTING.md#releases) without waiting: the TestPyPI
   run, then a GitHub Release tagged `vX.Y.Z` (not a prerelease, which VS Code
   skips). Check that PyPI, npm, Homebrew and both VS Code registries got it.
5. **Disclose.** Set the patched version in the advisory and publish it. GitHub
   reviews it, adds it to the GitHub Advisory Database and may alert
   dependants on PyPI and npm through Dependabot. Mention the advisory in the
   release notes, since Homebrew and VS Code users get no alert.
