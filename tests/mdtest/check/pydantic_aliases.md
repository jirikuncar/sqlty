# Pydantic aliases and extra fields

`schema.sql`:

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW()
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Field alias matches the column name

```python
from pydantic import BaseModel, Field
from sqlty import SQL

class User(BaseModel):
    user_id: int = Field(alias="id")
    name: str

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## A column named after the field is not accepted

```python
from pydantic import BaseModel, Field
from sqlty import SQL

class User(BaseModel):
    user_id: int = Field(alias="id")
    name: str

# error: [column-count-mismatch] "User has 2 required fields but query returns 1 columns"
query = SQL[User]("--sql\nSELECT name FROM users")
```

## The alias is required when the query omits it

```python
from pydantic import BaseModel, Field
from sqlty import SQL

class User(BaseModel):
    user_id: int = Field(alias="id")
    name: str

# error: [typeddict-key-mismatch] "User.id is required but missing from query"
query = SQL[User]("--sql\nSELECT user_id::INTEGER, name FROM users")
```

## validation_alias and Annotated metadata

```python
from typing import Annotated
from pydantic import BaseModel, Field
from sqlty import SQL

class User(BaseModel):
    user_id: Annotated[int, Field(validation_alias="id")]
    full_name: str = Field(validation_alias="name")

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## populate_by_name accepts either name

```python
from pydantic import BaseModel, ConfigDict, Field
from sqlty import SQL

class User(BaseModel):
    model_config = ConfigDict(populate_by_name=True)
    user_id: int = Field(alias="id")
    name: str

# error:
by_alias = SQL[User]("--sql\nSELECT id, name FROM users")
# error:
by_name = SQL[User]("--sql\nSELECT id AS user_id, name FROM users")
```

## Aliased field type is still checked

```python
from pydantic import BaseModel, Field
from sqlty import SQL

class User(BaseModel):
    user_id: bool = Field(alias="id")

# error: [column-type-mismatch] "User.user_id is bool but query column 'id' infers int"
query = SQL[User]("--sql\nSELECT id FROM users")
```

## AliasChoices and alias_generator leave the names unknown

```python
from pydantic import AliasChoices, BaseModel, ConfigDict, Field
from pydantic.alias_generators import to_camel
from sqlty import SQL

class Choice(BaseModel):
    user_id: int = Field(validation_alias=AliasChoices("id", "uid"))
    name: str

class Camel(BaseModel):
    model_config = ConfigDict(alias_generator=to_camel, extra="forbid")
    created_at: str

# error:
a = SQL[Choice]("--sql\nSELECT id, name FROM users")
# error:
b = SQL[Choice]("--sql\nSELECT name, email FROM users")
# error:
c = SQL[Camel]("--sql\nSELECT created_at, name FROM users")
```

## extra="forbid" rejects undeclared columns

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class User(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: int

# error: [typeddict-key-mismatch] "User has no field for query column 'name'"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Class Config with extra = "forbid"

```python
from pydantic import BaseModel
from sqlty import SQL

class User(BaseModel):
    id: int

    class Config:
        extra = "forbid"

# error: [typeddict-key-mismatch] "User has no field for query column 'name'"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## Other extra settings ignore undeclared columns

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class User(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: int

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```
