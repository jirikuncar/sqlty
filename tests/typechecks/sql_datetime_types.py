"""SQL[T] with datetime and uuid types.

mypy: ok
pyright: ok
ty: ok
"""

import datetime
import uuid

from sqlty import SQL

# datetime types in tuple
ts_query = SQL[tuple[int, datetime.datetime]]("--sql\nSELECT id::INTEGER, created_at::TIMESTAMP FROM events")
date_query = SQL[tuple[int, datetime.date]]("--sql\nSELECT id::INTEGER, event_date::DATE FROM events")
uuid_query = SQL[tuple[uuid.UUID, str]]("--sql\nSELECT token::UUID, name::TEXT FROM tokens")

reveal_type(ts_query)    # SQL[tuple[int, datetime]]
reveal_type(date_query)  # SQL[tuple[int, date]]
reveal_type(uuid_query)  # SQL[tuple[UUID, str]]
