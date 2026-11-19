# Receivers of a registered method

```console
$ sqlty check models.py
? 1
models.py:3: [unknown-column] column `nme` does not exist in `users`; did you mean `name`?
models.py:7: [unknown-column] column `idd` does not exist in `users`; did you mean `id`?
Found 2 error(s)

$ sqlty check bad/q.py
? 2
sqlty: error: [CWD]/bad/pyproject.toml: [[tool.sqlty.functions]] receivers must be an array of strings

```
