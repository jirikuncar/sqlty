from sqlty import SQL

q = SQL[tuple[int]]("""--sql
SELECT id
FROM users
JOIN orders ON orders.user_id = users.id""")
