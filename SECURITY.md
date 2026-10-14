# Security Policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Do not open a public issue. Report it privately through
[GitHub private vulnerability reporting](https://github.com/jirikuncar/sqlty/security/advisories/new).

Include the affected version, a minimal reproduction and the impact you expect.
Once a fix is released, the advisory is published with credit to the reporter
unless you ask otherwise.

## Scope

sqlty reads and rewrites Python source and parses SQL; it never executes the
queries it analyzes. Relevant reports include, for example, code execution
through crafted source, SQL or configuration, crashes of the Rust engine
(`sqlty._core`), and the `# coding: sqlty` codec or its `sqlty_magic.pth`
startup hook acting on files that did not opt in.
