from sqlty import SQL
by_email = SQL[tuple[int, str]]("""--sql
select
    id,
    email
from users
where email = %(email)s""")
dynamic = f"""--sql
select {column} from users"""
