from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT score FROM users")
