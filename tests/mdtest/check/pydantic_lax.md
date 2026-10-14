# Pydantic lax-mode conversions

`schema.sql`:

```sql
CREATE TABLE usage (
    id INTEGER PRIMARY KEY,
    records BIGINT NOT NULL,
    price NUMERIC(10, 2) NOT NULL,
    active INTEGER NOT NULL,
    day DATE NOT NULL,
    ratio DOUBLE PRECISION NOT NULL,
    created TIMESTAMP NOT NULL
);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## Lax mode accepts conversions Pydantic performs

```python
import datetime
from decimal import Decimal
from pydantic import BaseModel
from sqlty import SQL

class Usage(BaseModel):
    total: int
    price: Decimal
    active: bool
    day: datetime.datetime

# error:
query = SQL[Usage]("--sql\nSELECT sum(records) AS total, id AS price, active, day FROM usage GROUP BY id, active, day")
```

## Strict models still report them

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Usage(BaseModel):
    model_config = ConfigDict(strict=True)
    total: int

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

## Strict fields still report them

```python
from pydantic import BaseModel, Field
from sqlty import SQL

class Usage(BaseModel):
    total: int = Field(strict=True)

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

## TypedDict rows are not converted

```python
from typing import TypedDict
from sqlty import SQL

class Usage(TypedDict):
    total: int

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

## Conversions Pydantic rejects are still reported

```python
from pydantic import BaseModel
from sqlty import SQL

class Usage(BaseModel):
    total: str

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

## Each conversion, lax and strict

### Decimal to int

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: int

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: int

lax = SQL[Lax]("--sql\nSELECT price AS value FROM usage")
# error: [column-type-mismatch] "Strict.value is int but query column 'value' infers decimal.Decimal"
strict = SQL[Strict]("--sql\nSELECT price AS value FROM usage")
```

### float to int

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: int

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: int

lax = SQL[Lax]("--sql\nSELECT ratio AS value FROM usage")
# error: [column-type-mismatch] "Strict.value is int but query column 'value' infers float"
strict = SQL[Strict]("--sql\nSELECT ratio AS value FROM usage")
```

### int to Decimal

```python
from decimal import Decimal
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: Decimal

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: Decimal

lax = SQL[Lax]("--sql\nSELECT id AS value FROM usage")
# error: [column-type-mismatch] "Strict.value is Decimal but query column 'value' infers int"
strict = SQL[Strict]("--sql\nSELECT id AS value FROM usage")
```

### float to Decimal

```python
import decimal
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: decimal.Decimal

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: decimal.Decimal

lax = SQL[Lax]("--sql\nSELECT ratio AS value FROM usage")
# error: [column-type-mismatch] "Strict.value is decimal.Decimal but query column 'value' infers float"
strict = SQL[Strict]("--sql\nSELECT ratio AS value FROM usage")
```

### int to bool

```python
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: bool

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: bool

lax = SQL[Lax]("--sql\nSELECT active AS value FROM usage")
# error: [column-type-mismatch] "Strict.value is bool but query column 'value' infers int"
strict = SQL[Strict]("--sql\nSELECT active AS value FROM usage")
```

### date to datetime

```python
from datetime import datetime
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: datetime

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: datetime

lax = SQL[Lax]("--sql\nSELECT day AS value FROM usage")
# error: [column-type-mismatch] "Strict.value is datetime but query column 'value' infers datetime.date"
strict = SQL[Strict]("--sql\nSELECT day AS value FROM usage")
```

### datetime to date

A `date` field accepts a `datetime` column in strict mode too: `datetime` is a
subclass of `date`.

```python
import datetime as dt
from pydantic import BaseModel, ConfigDict
from sqlty import SQL

class Lax(BaseModel):
    value: dt.date

class Strict(BaseModel):
    model_config = ConfigDict(strict=True)
    value: dt.date

lax = SQL[Lax]("--sql\nSELECT created AS value FROM usage")
# error:
strict = SQL[Strict]("--sql\nSELECT created AS value FROM usage")
```

## Strict forms

### `class Config`

```python
from pydantic import BaseModel
from sqlty import SQL

class Usage(BaseModel):
    class Config:
        strict = True

    total: int

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

### `model_config` dict

```python
from pydantic import BaseModel
from sqlty import SQL

class Usage(BaseModel):
    model_config = {"strict": True}
    total: int

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

### `Field(strict=True)` in `Annotated`

```python
from typing import Annotated
from pydantic import BaseModel, Field
from sqlty import SQL

class Usage(BaseModel):
    total: Annotated[int, Field(strict=True)]

# error: [column-type-mismatch]
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

### `Strict()` in `Annotated`

```python
from typing import Annotated
from pydantic import BaseModel, Strict
from sqlty import SQL

class Usage(BaseModel):
    total: Annotated[int, Strict()]
    active: Annotated[bool, Strict(False)]

# error: [column-type-mismatch] "Usage.total is Annotated[int, Strict()] but query column 'total' infers decimal.Decimal"
query = SQL[Usage]("--sql\nSELECT sum(records) AS total, active FROM usage")
```

### `StrictInt`

```python
from pydantic import BaseModel, Field, StrictInt
from sqlty import SQL

class Usage(BaseModel):
    total: StrictInt | None = Field(default=None, strict=False)

# error: [column-type-mismatch] "Usage.total is StrictInt but query column 'total' infers decimal.Decimal"
query = SQL[Usage]("--sql\nSELECT sum(records) AS total FROM usage")
```

### `StrictBool`

```python
import pydantic
from sqlty import SQL

class Usage(pydantic.BaseModel):
    active: pydantic.StrictBool

# error: [column-type-mismatch] "Usage.active is pydantic.StrictBool but query column 'active' infers int"
query = SQL[Usage]("--sql\nSELECT active FROM usage")
```

### `StrictFloat`

```python
from pydantic import BaseModel
from pydantic.types import StrictFloat
from sqlty import SQL

class Usage(BaseModel):
    ratio: StrictFloat

# error: [column-type-mismatch] "Usage.ratio is StrictFloat but query column 'ratio' infers decimal.Decimal"
query = SQL[Usage]("--sql\nSELECT price AS ratio FROM usage")
```

### `StrictStr`

```python
from pydantic import BaseModel, StrictStr
from sqlty import SQL

class Usage(BaseModel):
    name: StrictStr

# error: [column-type-mismatch] "Usage.name is StrictStr but query column 'name' infers int"
query = SQL[Usage]("--sql\nSELECT id AS name FROM usage")
```

### A lax field in a strict model

```python
from pydantic import BaseModel, ConfigDict, Field
from sqlty import SQL

class Usage(BaseModel):
    model_config = ConfigDict(strict=True)
    total: int = Field(strict=False)
    active: bool

# error: [column-type-mismatch] "Usage.active is bool but query column 'active' infers int"
query = SQL[Usage]("--sql\nSELECT sum(records) AS total, active FROM usage")
```

## Pydantic dataclasses

```python
from pydantic import ConfigDict
from pydantic.dataclasses import dataclass
from sqlty import SQL

@dataclass
class Lax:
    total: int

@dataclass(config=ConfigDict(strict=True))
class Strict:
    total: int

@dataclass
class Configured:
    __pydantic_config__ = ConfigDict(strict=True)
    total: int

lax = SQL[Lax]("--sql\nSELECT sum(records) AS total FROM usage")
# error: [column-type-mismatch] "Strict.total is int but query column 'total' infers decimal.Decimal"
strict = SQL[Strict]("--sql\nSELECT sum(records) AS total FROM usage")
# error: [column-type-mismatch] "Configured.total is int but query column 'total' infers decimal.Decimal"
configured = SQL[Configured]("--sql\nSELECT sum(records) AS total FROM usage")
```

## Other row types are not converted

```python
from dataclasses import dataclass
from typing import NamedTuple
from sqlty import SQL

@dataclass
class Plain:
    total: int

class Tuple(NamedTuple):
    total: int

# error: [column-type-mismatch] "Plain.total is int but query column 'total' infers decimal.Decimal"
plain = SQL[Plain]("--sql\nSELECT sum(records) AS total FROM usage")
# error: [column-type-mismatch] "Tuple.total is int but query column 'total' infers decimal.Decimal"
named = SQL[Tuple]("--sql\nSELECT sum(records) AS total FROM usage")
```
