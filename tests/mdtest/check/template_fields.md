# t-string fields

Which t-string fields sqlty reads as part of a query, and the field
`tstring-skipped` names when it can't.

## Wire format specs

`:s`, `:b` and `:t` are placeholders without psycopg imported too.

```python
from sqlty import SQLTemplate

user_id = 1
# error:
q = SQLTemplate[tuple[int]](t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id:s} OR id = {user_id:t}")
```

## Other specs

```python
user_id = 1
# error: [tstring-skipped] "the field {user_id:>10} changes the SQL"
q = t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id:>10}"
# error: [tstring-skipped] "the field {user_id:{width}} changes the SQL"
r = t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id:{width}}"
# error: [tstring-skipped] "the field {user_id=} changes the SQL"
s = t"--sql\nSELECT id::INTEGER FROM users WHERE id = {user_id=}"
```

## psycopg identifiers

An identifier is read in a statement that returns no rows.

```python
from sqlty.drivers.psycopg import execute

table = "users"
user_id = 1
# error:
a = t"--sql\nUPDATE {table:i} SET name = 'x' WHERE id = {user_id}"
# error:
b = t"--sql\nDELETE FROM {table:i} WHERE id = {user_id}"
# error:
c = t"--sql\nNOTIFY {table:i}, {user_id:l}"
# error: [tstring-skipped] "the field {table:i} changes the SQL"
d = t"--sql\nDELETE FROM {table:i} WHERE id = {user_id} RETURNING id"
# error: [tstring-skipped] "the field {table:i} changes the SQL"
e = t"--sql\nSELECT id::INTEGER FROM {table:i}"
```

## psycopg snippets

A snippet is inlined when it names a template assigned once at module level;
its fields are read the same way.

```python
import psycopg
from sqlty import SQLTemplate

column = "id"
user_id = 1
by_id = t"id = {user_id}"
by_column = t"{column:i} = {user_id}"
twice = t"id = 1"
twice = t"id = 2"
looped = t"id = {looped:q}"
plain = "id = 1"
# error:
a = SQLTemplate[tuple[int]](t"--sql\nSELECT id::INTEGER FROM users WHERE {by_id:q}")
# error: [tstring-skipped] "the field {by_column:q} changes the SQL"
b = t"--sql\nSELECT id::INTEGER FROM users WHERE {by_column:q}"
# error: [tstring-skipped] "the field {twice:q} changes the SQL"
c = t"--sql\nSELECT id::INTEGER FROM users WHERE {twice:q}"
# error: [tstring-skipped] "the field {looped:q} changes the SQL"
d = t"--sql\nSELECT id::INTEGER FROM users WHERE {looped:q}"
# error: [tstring-skipped] "the field {plain:q} changes the SQL"
e = t"--sql\nSELECT id::INTEGER FROM users WHERE {plain:q}"
```
