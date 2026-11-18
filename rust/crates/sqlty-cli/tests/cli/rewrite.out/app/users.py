import typing
from sqlty import SQL


# sqlty: generated
class ByEmailParams(typing.TypedDict):
    email: str | None


by_email = SQL[tuple[int, str], ByEmailParams]("""--sql
select
    id,
    email
from users
where email = %(email)s""")
dynamic = f"""--sql
select {column} from users"""
