# Complete

`sqlty complete FILE:LINE:COL` lists the names that complete at a cursor, one per line:
the label, its kind, the detail and the SQL type, separated by tabs.

```console
$ sqlty complete q.py:1:36
orders	table	public	

$ sqlty complete q.py:2:32
id	column	active_users	INT

$ sqlty complete q.py:3:70
recent	cte		

$ sqlty complete q.py:4:45
users ON users.id = orders.user_id	join	public	
users	table	public	

```

## Columns count characters

The cursor is in characters, so the `é` before it takes one column, and `--json` gives the
span the completion replaces in the same unit.

```console
$ sqlty complete q.py:5:32
name	column	users	TEXT

$ sqlty complete --json q.py:5:32
{"end":{"column":32,"line":5},"incomplete":false,"items":[{"detail":"users","kind":"column","label":"name","nullable":true,"sql_type":"TEXT"}],"start":{"column":30,"line":5}}

```

## A cursor past the text

A column past the end of its line stops at the line break, and a line past the last is the
last; line 0 and an empty file have nothing to complete.

```console
$ sqlty complete q.py:1:99
$ sqlty complete wide.py:2:99
orders	table	public	

$ sqlty complete multi.py:99:9
orders	table	public	

$ sqlty complete q.py:0:0
$ sqlty complete empty.py:1:1

```

## Errors

```console
$ sqlty complete app.py:12
? 2
error: invalid value 'app.py:12' for '<FILE:LINE:COL>': expected FILE:LINE:COL, got 'app.py:12'

For more information, try '--help'.

$ sqlty complete :1:2
? 2
error: invalid value ':1:2' for '<FILE:LINE:COL>': expected FILE:LINE:COL, got ':1:2'

For more information, try '--help'.

$ sqlty complete app.py:a:2
? 2
error: invalid value 'app.py:a:2' for '<FILE:LINE:COL>': expected FILE:LINE:COL, got 'app.py:a:2'

For more information, try '--help'.

$ sqlty complete app.py:1:+2
? 2
error: invalid value 'app.py:1:+2' for '<FILE:LINE:COL>': expected FILE:LINE:COL, got 'app.py:1:+2'

For more information, try '--help'.

$ sqlty complete C:/app.py:12:30
? 2
sqlty: error: C:/app.py: no such file

$ sqlty complete .:1:1
? 2
sqlty: error: .: no such file

```
