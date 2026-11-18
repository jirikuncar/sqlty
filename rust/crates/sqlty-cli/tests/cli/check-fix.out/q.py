from sqlty import SQL

stale = SQL[tuple[int]]("""--sql
SELECT id FROM users""")
star = SQL[tuple[int, str]]("""--sql
SELECT id::INT, name::TEXT FROM users""")
new = SQL[tuple[str]]("""--sql
SELECT name FROM users""")
dynamic = f"""--sql
SELECT {column} FROM users"""
