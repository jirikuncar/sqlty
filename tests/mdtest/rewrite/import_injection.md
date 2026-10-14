# Import Injection

## SQL import is added

```python
q = "--sql\nSELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Datetime import is added

```python
q = "--sql\nSELECT created_at::TIMESTAMP FROM events"
# rewritten: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM events")
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM events")
```

## UUID import is added

```python
q = "--sql\nSELECT user_id::UUID FROM users"
# rewritten: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID FROM users")
# rewritten-file: import uuid
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID FROM users")
```

## Existing import is not duplicated

```python
from sqlty import SQL
q = "--sql\nSELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Existing sqlty imports are recognised

`SQL` and `UNTYPED` count as imported in any top-level `from sqlty import`
statement, parenthesised or not, and `*` imports both. A missing name gets a
line of its own, which `strip` removes again.

### In a list

```python
from sqlty import SQL, UNTYPED
q = SQL[tuple[str]]("--sql\nSELECT 1::INTEGER AS n")
r = "--sql\nSELECT 2::INTEGER AS m"
# fixed: from sqlty import SQL, UNTYPED
# fixed: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
# fixed: r = SQL[tuple[int]]("--sql\nSELECT 2::INTEGER AS m")
```

### Parenthesised over lines

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
from sqlty import (  # see (docs)
    SQL,  # the wrapper
    UNTYPED,
)
q = "--sql\nSELECT x, y FROM pairs"
# rewritten-file: from sqlty import (  # see (docs)
# rewritten-file:     SQL,  # the wrapper
# rewritten-file:     UNTYPED,
# rewritten-file: )
# rewritten-file: q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

### Star import

```toml
[tool.sqlty]
untyped = "sentinel"
```

```python
from sqlty import *
q = "--sql\nSELECT x, y FROM pairs"
# rewritten-file: from sqlty import *
# rewritten-file: q = SQL[UNTYPED]("--sql\nSELECT x, y FROM pairs")
```

### Missing names get their own line

```python
from sqlty import UNTYPED; x = 1
q = "--sql\nSELECT 1::INTEGER AS n"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import UNTYPED; x = 1
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
# stripped: from sqlty import UNTYPED; x = 1
# stripped: q = "--sql\nSELECT 1::INTEGER AS n"
```

### Alias of another name

`SQL as S` does not bind `SQL`.

```python
from sqlty import SQL as S
q = "--sql\nSELECT 1::INTEGER AS n"
# rewritten-file: from sqlty import SQL
# rewritten-file: from sqlty import SQL as S
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
```

### Alias from the import style

```toml
[tool.sqlty]
import-style = "from sqlty import SQL as Q"
```

```python
from sqlty import (
    SQL as Q,
)
q = "--sql\nSELECT 1::INTEGER AS n"
# rewritten-file: from sqlty import (
# rewritten-file:     SQL as Q,
# rewritten-file: )
# rewritten-file: q = Q[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
```

### Text in strings and comments is no import

```python
"""Example:
from sqlty import UNTYPED
"""
# from sqlty import SQL
q = "--sql\nSELECT 1::INTEGER AS n"
# rewritten-file: """Example:
# rewritten-file: from sqlty import UNTYPED
# rewritten-file: """
# rewritten-file: from sqlty import SQL
# rewritten-file: # from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
```

## Placement after the module header

Imports go below the shebang, the coding cookie, the module docstring and
`from __future__` imports, which must stay first.

```python
#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""Queries."""

from __future__ import annotations

q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: #!/usr/bin/env python
# rewritten-file: # -*- coding: utf-8 -*-
# rewritten-file: """Queries."""
# rewritten-file:
# rewritten-file: from __future__ import annotations
# rewritten-file: from sqlty import SQL
# rewritten-file:
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Other leading comments stay above their code

```python
# Count users
q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: from sqlty import SQL
# rewritten-file: # Count users
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Import mentioned in the docstring is not an import

```python
"""Usage:

    from sqlty import SQL
"""
q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: """Usage:
# rewritten-file:
# rewritten-file:     from sqlty import SQL
# rewritten-file: """
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Indented import is not a top-level import

```python
if TYPE_CHECKING:
    from sqlty import SQL
q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: from sqlty import SQL
# rewritten-file: if TYPE_CHECKING:
# rewritten-file:     from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Import after a compound statement on one line is not top-level

After `;` on a line that starts a compound statement, the import is part of
its body.

```python
from typing import TYPE_CHECKING
if TYPE_CHECKING: pass; from sqlty import SQL
q = "--sql\nSELECT 1::INTEGER AS n"
# rewritten-file: from sqlty import SQL
# rewritten-file: from typing import TYPE_CHECKING
# rewritten-file: if TYPE_CHECKING: pass; from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER AS n")
# stripped: from typing import TYPE_CHECKING
# stripped: if TYPE_CHECKING: pass; from sqlty import SQL
# stripped: q = "--sql\nSELECT 1::INTEGER AS n"
```

## Added imports use the file's line ending

In a file with CRLF line endings, `rewrite` and `check --fix` end added
import lines with CRLF too. `tests/test_core_invariants.py` runs every
example in these docs with CRLF line endings as well.

```python
"""Events."""
from sqlty import SQL
q = SQL[tuple[str]]("--sql\nSELECT NOW()::TIMESTAMP AS t")
# fixed: """Events."""
# fixed: import datetime
# fixed: from sqlty import SQL
# fixed: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT NOW()::TIMESTAMP AS t")
```
