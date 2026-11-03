from sqlty import SQL

q = SQL[tuple[int, str]]("--sql\nSELECT id, surname FROM users")
