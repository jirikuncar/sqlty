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

$ sqlty check --fix -c 'format-command = ["sh", "-c", "echo bad layout >&2; exit 1"]' s.py
format-command sh failed on s.py: bad layout
Fixed 1 file(s)

$ sqlty check --fix
q.py:9: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 1 warning(s)

```
