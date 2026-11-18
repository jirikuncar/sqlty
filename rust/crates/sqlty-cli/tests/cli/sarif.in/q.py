from sqlty import SQL

q = SQL[tuple[str]]("""--sql
SELECT id FROM users""")
