"""Example: typed parameters.

``sqlty rewrite`` writes the type of the parameters of each query next to its
rows, ``SQL[R, P]``: a ``tuple`` for positional parameters, a generated
``TypedDict`` for named ones.

Run ``sqlty strip .`` to reverse.
"""

import datetime
import typing
from sqlty import SQL


# sqlty: generated
class ByIdParams(typing.TypedDict):
    id: int | None


by_id = SQL[tuple[str, str | None], ByIdParams](
    "--sql\nSELECT name, email FROM users WHERE id = %(id)s"
)
born_after = SQL[tuple[int, str], tuple[datetime.date | None]](
    "--sql\nSELECT id, name FROM users WHERE born > %s"
)
rename = "--sql\nUPDATE users SET name = %s WHERE id = %s"
