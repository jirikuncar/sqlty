from sqlty import SQL

q = SQL[tuple[int]]("""--sql
SELECT id FROM users""")
