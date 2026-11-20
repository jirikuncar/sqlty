from typing import ClassVar

from . import Postgres

class Model:
    db: ClassVar[Postgres]
