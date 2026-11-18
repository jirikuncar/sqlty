from sqlty import SQL

ids = SQL[tuple[int]]("""--sql
SELECT id FROM users""")
nope = """--sql
SELECT nope FROM users"""
total = SQL[tuple[str]]("""--sql
SELECT SUM(qty) FROM users""")
star = """--sql
SELECT * FROM users"""
dynamic = f"""--sql
SELECT {column} FROM users"""
