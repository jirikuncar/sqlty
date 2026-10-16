# Imported row types

A row type imported from another checked module is checked like one defined in
the file. The module is read from its source, never imported.

```toml
[tool.sqlty]
dialect = "postgres"
```

`pkg/models.py`:

```python
from dataclasses import dataclass
from typing import NamedTuple, TypedDict

from pydantic import BaseModel

class User(TypedDict):
    id: str
    name: str

class UserRow(NamedTuple):
    id: int
    name: str

@dataclass
class UserData:
    id: int

class UserModel(BaseModel):
    id: int
    name: str
```

`pkg/__init__.py`:

```python
from .models import User as Person
```

## From imports, module imports and aliases

```python
import pkg.models as m
from pkg.models import User, UserData, UserModel
from pkg.models import UserRow as Row
from sqlty import SQL

# error: [column-type-mismatch] "User.id is str but query column 'id' infers int"
a = SQL[User]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name")
# error: [column-type-mismatch] "m.User.id is str but query column 'id' infers int"
b = SQL[m.User]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name")
# error: [column-count-mismatch] "Row has 2 fields but query returns 1 columns"
c = SQL[Row]("--sql\nSELECT 1::INTEGER AS id")
# error: [column-count-mismatch] "UserData has 1 fields but query returns 2 columns"
d = SQL[UserData]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name")
# error: [column-type-mismatch] "UserModel.name is str but query column 'name' infers int"
e = SQL[UserModel]("--sql\nSELECT 1::INTEGER AS id, 2::INTEGER AS name")
ok = SQL[Row]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name")
```

## Package re-exports

```python
import pkg
from pkg import Person
from sqlty import SQL

# error: [column-type-mismatch] "Person.id is str but query column 'id' infers int"
a = SQL[Person]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name")
# error: [column-type-mismatch] "pkg.Person.id is str but query column 'id' infers int"
b = SQL[pkg.Person]("--sql\nSELECT 1::INTEGER AS id, 'x'::TEXT AS name")
```

## Unresolvable types stay silent

```python
from elsewhere import Row
from pkg.models import Missing
from sqlty import SQL

# error:
a = SQL[Row]("--sql\nSELECT 1::INTEGER AS id")
# error:
b = SQL[Missing]("--sql\nSELECT 1::INTEGER AS id")
```
