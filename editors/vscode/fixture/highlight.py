from sqlty import SQL

marked = "--sql\nSELECT name, 'it''s' FROM users WHERE id = %(id)s -- note"
single = '-- sql\nSELECT 1 AS "one" FROM users'
wrapped = SQL[tuple[int]]("--sql\nSELECT id FROM users WHERE id > $1")
triple = """--sql
SELECT id /* the
key */ FROM users"""

plain = "SELECT plain FROM users"
formatted = f"--sql\nSELECT {plain} FROM users"
raw = r"--sql\nSELECT raw FROM users"
data = b"--sql\nSELECT data FROM users"

user_id, limit = 1, 10
templated = t"--sql\nSELECT name FROM users WHERE id = {user_id} LIMIT {limit}"
legacy = f"""--sql
SELECT '{{literal}}', {limit} FROM users"""
raw_template = rt"--sql\nSELECT unmarked_template FROM users"
raw_formatted = rf"--sql\nSELECT unmarked_formatted FROM {plain}"
