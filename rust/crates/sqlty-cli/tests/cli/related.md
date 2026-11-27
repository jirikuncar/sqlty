# Related locations

An ambiguous column is noted at each table that has it.

```console
$ sqlty check
? 1
app/queries.py:3: [ambiguous-column] column reference `id` is ambiguous: `users`, `orders` all have it
  app/queries.py:5: note: column `id` is here
  app/queries.py:6: note: column `id` is here
Found 1 error(s)

$ sqlty check --output-format json
? 1
[
  {
    "file": "app/queries.py",
    "rule": "ambiguous-column",
    "message": "column reference `id` is ambiguous: `users`, `orders` all have it",
    "severity": "error",
    "line": 3,
    "start": {
      "line": 4,
      "column": 8
    },
    "end": {
      "line": 4,
      "column": 10
    },
    "fix": [],
    "related": [
      {
        "message": "column `id` is here",
        "start": {
          "line": 5,
          "column": 6
        },
        "end": {
          "line": 5,
          "column": 11
        }
      },
      {
        "message": "column `id` is here",
        "start": {
          "line": 6,
          "column": 6
        },
        "end": {
          "line": 6,
          "column": 12
        }
      }
    ]
  }
]

```
