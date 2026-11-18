from sqlty import SQL

stale = SQL[tuple[str]]("""--sql
SELECT id FROM users""")
star = SQL[tuple[int, str]]("""--sql
SELECT * FROM users""")
new = """--sql
SELECT name FROM users"""
dynamic = f"""--sql
SELECT {column} FROM users"""
