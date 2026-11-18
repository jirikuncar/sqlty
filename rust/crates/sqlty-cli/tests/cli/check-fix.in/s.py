from sqlty import SQL

q = SQL[tuple[int]]("""--sql
SELECT name FROM users WHERE id = 1""")
