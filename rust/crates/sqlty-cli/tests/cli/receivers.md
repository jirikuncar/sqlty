# Receivers of helper methods

`app` is checked. `lib` is described by a stub under `stub-paths`, `Cached` inherits its
helper from a class of another checked module, and `vendored` is a library read from the
project directory. `typings` is not a directory of `stub-paths` here, so `other` is not
read.

```console
$ sqlty check app
? 1
app/queries.py:8: [unknown-column] column `nope` does not exist in `users`
app/queries.py:12: [column-type-mismatch] int is expected but query column 'name' infers str
app/queries.py:16: [column-count-mismatch] int takes 1 column but query returns 2 columns
app/queries.py:25: [unknown-column] column `missing` does not exist in `users`
Found 4 error(s)

```
