# Diff and quiet

`--check --quiet` prints nothing and only the exit code tells that a file would change.
A diff marks a last line that has no newline, on both sides.

```console
$ sqlty rewrite --check --quiet
? 1

$ sqlty rewrite --diff
? 1
--- q.py
+++ q.py
@@ -1,3 +1,4 @@
+from sqlty import SQL
 a = 1
-q = """--sql
-SELECT id FROM users"""
/ No newline at end of file
+q = SQL[tuple[int]]("""--sql
+SELECT id FROM users""")
/ No newline at end of file

```
