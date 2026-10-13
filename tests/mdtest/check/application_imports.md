# Application imports are not executed

```toml
[tool.sqlty]
dialect = "postgres"
```

## Unknown third-party name stays unknown without importing it

```python
from typing import TypedDict
from some_app.ids import UserId
from sqlty import SQL

class User(TypedDict):
    id: UserId

# error:
query = SQL[User]("--sql\nSELECT id::INTEGER FROM users")
```

## Standard library and typing packages are still resolved

```python
from datetime import datetime
from typing import Annotated, TypedDict
from pydantic import Field
from sqlty import SQL

class Event(TypedDict):
    id: Annotated[int, Field(ge=0)]
    at: datetime

# error: [column-type-mismatch] "Event.at is datetime but query column 'at' infers str"
query = SQL[Event]("--sql\nSELECT id::INTEGER, at::TEXT FROM events")
```
