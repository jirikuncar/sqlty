# Check

```console
$ sqlty check
? 1
Schema pattern matches no files: none/*.sql
legacy/latin1.py: not UTF-8 (invalid start byte at byte 0); skipped
app/queries.py:5: [unknown-column] column `nope` does not exist in `users`
app/queries.py:7: [narrow-type] SQL[tuple[str]] should be SQL[tuple[int | None]]
app/queries.py:9: [stale-rewrite] Source has --sql strings that aren't wrapped in SQL[T]
app/queries.py:11: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 2 error(s), 2 warning(s)

$ sqlty check --output-format json --ignore stale-rewrite app/queries.py
? 1
Schema pattern matches no files: none/*.sql
[
  {
    "file": "app/queries.py",
    "rule": "unknown-column",
    "message": "column `nope` does not exist in `users`",
    "severity": "error",
    "line": 5,
    "start": {
      "line": 6,
      "column": 8
    },
    "end": {
      "line": 6,
      "column": 12
    },
    "fix": []
  },
  {
    "file": "app/queries.py",
    "rule": "narrow-type",
    "message": "SQL[tuple[str]] should be SQL[tuple[int | None]]",
    "severity": "error",
    "line": 7,
    "start": {
      "line": 7,
      "column": 13
    },
    "end": {
      "line": 7,
      "column": 23
    },
    "fix": [
      {
        "start": {
          "line": 7,
          "column": 13
        },
        "end": {
          "line": 7,
          "column": 23
        },
        "replacement": "tuple[int | None]"
      }
    ]
  },
  {
    "file": "app/queries.py",
    "rule": "fstring-skipped",
    "message": "f-string with --sql prefix skipped (f-strings cannot be statically rewritten)",
    "severity": "warn",
    "line": 11,
    "start": {
      "line": 11,
      "column": 11
    },
    "end": {
      "line": 12,
      "column": 30
    },
    "fix": []
  }
]

$ sqlty check --output-format github app/queries.py
? 1
Schema pattern matches no files: none/*.sql
::error file=app/queries.py,line=5,title=sqlty unknown-column::column `nope` does not exist in `users`
::error file=app/queries.py,line=7,title=sqlty narrow-type::SQL[tuple[str]] should be SQL[tuple[int | None]]
::warning file=app/queries.py,line=9,title=sqlty stale-rewrite::Source has --sql strings that aren't wrapped in SQL[T]
::warning file=app/queries.py,line=11,title=sqlty fstring-skipped::f-string with --sql prefix skipped (f-strings cannot be statically rewritten)

$ sqlty check --explain --statistics app/queries.py
? 1
Schema pattern matches no files: none/*.sql
app/queries.py:5: [unknown-column] column `nope` does not exist in `users`
app/queries.py:7: [narrow-type] SQL[tuple[str]] should be SQL[tuple[int | None]]
  sum: type and nullability by inference rule postgres.aggregate.sum-integers
    Summing small integers widens to bigint so the sum fits.
    Reference: PostgreSQL 17 documentation, 9.21, Table 9.60 General-Purpose Aggregate Functions (sum ( smallint ) → bigint, sum ( integer ) → bigint)
app/queries.py:9: [stale-rewrite] Source has --sql strings that aren't wrapped in SQL[T]
app/queries.py:11: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 2 error(s), 2 warning(s)
Queries: 3 typed, 1 partially typed, 0 unparsed, 1 skipped (fstring-skipped: 1)

$ sqlty check --exit-zero -q app
Schema pattern matches no files: none/*.sql

$ sqlty check --ignore unknown-column --ignore narrow-type app
Schema pattern matches no files: none/*.sql
app/queries.py:9: [stale-rewrite] Source has --sql strings that aren't wrapped in SQL[T]
app/queries.py:11: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 2 warning(s)

$ sqlty check --ignore unknown-column --ignore narrow-type --error-on-warning app
? 1
Schema pattern matches no files: none/*.sql
app/queries.py:9: [stale-rewrite] Source has --sql strings that aren't wrapped in SQL[T]
app/queries.py:11: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 2 warning(s)

$ sqlty check legacy
? 2
Schema pattern matches no files: none/*.sql
legacy/latin1.py: not UTF-8 (invalid start byte at byte 0); skipped

$ sqlty check --exit-zero --error-on-warning app
? 2
sqlty: error: --error-on-warning cannot be used with --exit-zero

$ sqlty check gone.py
? 2
sqlty: error: path does not exist: gone.py

```
