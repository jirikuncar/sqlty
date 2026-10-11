# typeddict-inheritance

Default severity: **error**

## What it does

Checks that a TypedDict subclass respects its bases' openness
([PEP 728](https://peps.python.org/pep-0728/)): it may not add keys to a
closed base, reopen it, or change a base's `extra_items` in a way the PEP
forbids. Reported on the subclass's `class` line.

## Why is this bad?

Type checkers reject such a class, and the row type sqlty validates no longer
means what its base promises.

## Example

```python
from typing import TypedDict

from sqlty import SQL


class Base(TypedDict, closed=True):
    id: int


# error: [typeddict-inheritance] "User cannot add key 'name' to closed base Base"
class User(Base):
    name: str


q = SQL[User]("--sql\nSELECT id::INTEGER, name::TEXT FROM users")
```
