# Check and fix

```console
$ sqlty check --fix -v q.py
Fixed q.py
q.py:9: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Fixed 1 file(s)
Found 1 warning(s)

$ sqlty check --fix -v --output-format json r.py
Fixed r.py
[]

$ sqlty check --fix -c 'format-command = ["false"]' s.py
format-command false failed on s.py: 
Fixed 1 file(s)

$ sqlty check --fix
q.py:9: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 1 warning(s)

```
