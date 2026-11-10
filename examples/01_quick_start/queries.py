from sqlty import SQL
from sqlty import NoParams

# Write SQL queries with --sql prefix and explicit type casts.
# Then run: sqlty rewrite examples/01_quick_start/

user_query = SQL[tuple[int, str], NoParams]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")

count_query = SQL[tuple[int], NoParams]("--sql\nSELECT COUNT(*)::INTEGER FROM users")

active_query = SQL[tuple[str, bool], NoParams](
    "--sql\nSELECT email::TEXT, active::BOOLEAN FROM users"
)
