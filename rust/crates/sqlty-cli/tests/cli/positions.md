# Positions

Columns count characters, so the text before a diagnostic never moves it, whether its line
spans one block of bytes or several.

```console
$ sqlty check --output-format json wide.py
? 1
[
  {
    "file": "wide.py",
    "rule": "unknown-column",
    "message": "column `nope` does not exist in `users`",
    "severity": "error",
    "line": 2,
    "start": {
      "line": 2,
      "column": 36
    },
    "end": {
      "line": 2,
      "column": 40
    },
    "fix": []
  },
  {
    "file": "wide.py",
    "rule": "unknown-column",
    "message": "column `nope2` does not exist in `users`",
    "severity": "error",
    "line": 3,
    "start": {
      "line": 3,
      "column": 331
    },
    "end": {
      "line": 3,
      "column": 336
    },
    "fix": []
  }
]

```
