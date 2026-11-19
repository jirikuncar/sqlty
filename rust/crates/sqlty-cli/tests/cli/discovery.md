# Discovery

```console
$ sqlty check
? 1
app/q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 error(s)

$ sqlty check --exclude app
? 1
node_modules/q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 error(s)

$ sqlty check generated node_modules
? 1
generated/q.py:1: [unknown-column] column `nope` does not exist in `users`
node_modules/q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 2 error(s)

$ sqlty check --force-exclude generated
No Python files found

$ sqlty check empty
No Python files found

$ sqlty rewrite empty
No Python files found

$ sqlty check --quiet empty
$ sqlty rewrite --quiet empty
$ sqlty check --statistics app
? 1
app/q.py:1: [unknown-column] column `nope` does not exist in `users`
Found 1 error(s)
Queries: 0 typed, 1 partially typed, 0 unparsed, 0 skipped

```
