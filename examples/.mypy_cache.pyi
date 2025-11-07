from typing import Literal, overload
from sqlty import SQL

@overload
def Query(query: str) -> str: ...
