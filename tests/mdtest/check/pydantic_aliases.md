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

# error: [typeddict-key-mismatch] "User has no field for query column 'user_id'"
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
    user_id: str = Field(alias="id")

# error: [column-type-mismatch] "User.user_id is str but query column 'id' infers int"
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

## extra="ignore" reports undeclared columns

`ignore` is the default: the column is fetched and dropped.

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class User(BaseModel):
    model_config = ConfigDict(extra="ignore")
    id: int

# error: [typeddict-key-mismatch] "User has no field for query column 'name'"
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## extra="allow" keeps undeclared columns

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class User(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: int

# error:
query = SQL[User]("--sql\nSELECT id, name FROM users")
```

## A config not read from source may allow undeclared columns

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

CONFIG = ConfigDict(extra="allow")

class Shared(BaseModel):
    model_config = CONFIG
    id: int

class Spread(BaseModel):
    model_config = ConfigDict(**CONFIG)
    id: int

class Base(BaseModel):
    model_config = ConfigDict(extra="allow")

class Child(Base):
    id: int

# error:
a = SQL[Shared]("--sql\nSELECT id, name FROM users")
b = SQL[Spread]("--sql\nSELECT id, name FROM users")
c = SQL[Child]("--sql\nSELECT id, name FROM users")
```

## Extra keys of a JSON object built into a model

A model nested in a JSON column reads its keys as a row reads columns: by alias, with
undeclared keys reported unless the model allows them.

```python
from typing import Annotated, LiteralString
from pydantic import BaseModel, ConfigDict, Field
from sqlty import SQL

def fetch_one[T](query: Annotated[LiteralString, SQL], form: type[T], /) -> T: ...

class Info(BaseModel):
    name: str

class Open(BaseModel):
    model_config = ConfigDict(extra="allow")
    name: str

class Aliased(BaseModel):
    user_name: str = Field(alias="name")

class Row(BaseModel):
    info: Info

class OpenRow(BaseModel):
    info: Open

class AliasedRow(BaseModel):
    info: Aliased

# error: [json-shape-mismatch] "Row.info is Info but query column 'info' builds JSON that does not fit: info has key 'password_hash' that Info does not declare"
extra = fetch_one("SELECT jsonb_build_object('name', 'a'::TEXT, 'password_hash', 'x'::TEXT) AS info", Row)
# error: [json-shape-mismatch] "Row.info is Info but query column 'info' builds JSON that does not fit: info has key 'nick' that Info does not declare; info.name is required by Info but the query does not build it"
missing = fetch_one("SELECT jsonb_build_object('nick', 'a'::TEXT) AS info", Row)
allowed = fetch_one("SELECT jsonb_build_object('name', 'a'::TEXT, 'password_hash', 'x'::TEXT) AS info", OpenRow)
aliased = fetch_one("SELECT jsonb_build_object('name', 'a'::TEXT) AS info", AliasedRow)
# error: [typeddict-key-mismatch] "Info has no field for query column 'id'"
# error: [typeddict-key-mismatch] "Info has no field for query column 'password_hash'"
user = fetch_one("SELECT 1::INTEGER AS id, 'a'::TEXT AS name, 'x'::TEXT AS password_hash", Info)
```
