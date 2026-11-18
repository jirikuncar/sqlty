# Rewrite, format and strip

```console
$ sqlty rewrite --check
? 1
app/users.py:3: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Would rewrite app/users.py
1 file(s) would be rewritten

$ sqlty rewrite --diff
? 1
app/users.py:3: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
--- app/users.py
+++ app/users.py
@@ -1,4 +1,5 @@
-by_email = """--sql
-select id, email from users where email = %(email)s"""
+from sqlty import SQL
+by_email = SQL[tuple[int, str]]("""--sql
+select id, email from users where email = %(email)s""")
 dynamic = f"""--sql
 select {column} from users"""

$ sqlty rewrite -v
app/users.py:3: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Rewrote app/users.py

$ sqlty check
app/users.py:4: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
Found 1 warning(s)

$ sqlty format --diff app
? 1
--- app/users.py
+++ app/users.py
@@ -1,5 +1,9 @@
 from sqlty import SQL
 by_email = SQL[tuple[int, str]]("""--sql
-select id, email from users where email = %(email)s""")
+select
+    id,
+    email
+from users
+where email = %(email)s""")
 dynamic = f"""--sql
 select {column} from users"""

$ sqlty format app
$ sqlty strip --diff
? 1
--- app/users.py
+++ app/users.py
@@ -1,9 +1,8 @@
-from sqlty import SQL
-by_email = SQL[tuple[int, str]]("""--sql
+by_email = """--sql
 select
     id,
     email
 from users
-where email = %(email)s""")
+where email = %(email)s"""
 dynamic = f"""--sql
 select {column} from users"""

$ sqlty strip
$ sqlty rewrite -c 'format-command = ["sh", "-c", "echo bad layout >&2; exit 1"]'
app/users.py:7: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
format-command sh failed on app/users.py: bad layout

$ sqlty strip
$ sqlty rewrite --config 'format-command = ["/nonexistent/fmt"]'
app/users.py:7: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
format-command /nonexistent/fmt failed on app/users.py: No such file or directory (os error 2)

$ sqlty rewrite --check
app/users.py:8: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)
All files up to date

```
