# Check and fix

```console
$ sqlty check --fix -v
Fixed q.py
q.py:9: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Fixed 1 file(s)
Found 1 warning(s)

$ sqlty check
q.py:9: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 1 warning(s)

```
