from sqlty import SQL

q = SQL[tuple[str]]("""--sql
SELECT name FROM users WHERE id = 1""")
