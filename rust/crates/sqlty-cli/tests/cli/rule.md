# Rules

````console
$ sqlty rule
column-count-mismatch           error  [..]
...
$ sqlty rule chasm-join
# chasm-join
...
$ sqlty rule add-required-column
# add-required-column
...
[`insert-missing-column`](https://sqlty.kuncar.dev/reference/rules/insert-missing-column/).
...
scan as [`constraint-scan`](https://sqlty.kuncar.dev/reference/rules/constraint-scan/) shows.
...
Judged by [`sqlty breaking`](https://sqlty.kuncar.dev/guide/breaking-changes/#migrations-that-lock-tables)
...
[schema](https://sqlty.kuncar.dev/guide/schema-inference/) files that the `--against` baseline does
...

$ sqlty rule unknown-colum
? 2
Unknown rule 'unknown-colum'; run `sqlty rule` for the list

````
