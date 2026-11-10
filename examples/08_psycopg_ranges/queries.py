import datetime

import psycopg.types.range

from sqlty import SQL
from sqlty import NoParams

# Without `driver = "psycopg"`, a range column has no Python type: unknown-type.
bookings = SQL[tuple[int, str, psycopg.types.range.Range[datetime.datetime]], NoParams]("""--sql
SELECT id, room, during FROM bookings""")

# lower() and upper() return the element type; an empty range has neither bound.
first_nights = SQL[tuple[int, datetime.date | None], NoParams]("""--sql
SELECT id, lower(nights) FROM bookings""")
