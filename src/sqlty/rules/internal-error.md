# internal-error

Default severity: **error**

## What it does

Reports a file sqlty crashed on, in place of its other diagnostics. The rest of
the project is still checked.

## Why is this bad?

Nothing in the file was checked, and the crash is a bug in sqlty.

## Example

```text
app.py:1: [internal-error] sqlty crashed checking this file: index out of bounds; please report it at https://github.com/jirikuncar/sqlty/issues/new?template=bug_report.yml&title=...
```

The link opens a bug report with the cause, version and platform filled in. Add
the file, or the smallest part of it that still crashes.
