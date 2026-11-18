# Databases and query files

```console
$ sqlty rewrite app
$ sqlty check -v
? 1
sql/latin1.sql: not UTF-8; skipped
app/reports/events.py:2: [select-star] SELECT * used — expand to explicit column list
app/reports/legacy.py:2: [select-star] SELECT * used — expand to explicit column list
app/users.py:2: [select-star] SELECT * used — expand to explicit column list
sql/events.sql:2: [unknown-column] column `nope` does not exist in `events`
sql/events.sql: get_kinds() -> unknown
sql/events.sql: by_kind(kind: str) -> tuple[str]
Found 4 error(s)

$ sqlty check sql
? 1
sql/latin1.sql: not UTF-8; skipped
sql/events.sql:2: [unknown-column] column `nope` does not exist in `events`
Found 1 error(s)

```
