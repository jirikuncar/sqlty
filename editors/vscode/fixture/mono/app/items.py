from sqlty import SQL

q = SQL[tuple[int]]("--sql\nSELECT price FROM items")
