from sqlty import SQL
q = SQL[tuple[int, str | None]]("""--sql
SELECT * FROM users""")
