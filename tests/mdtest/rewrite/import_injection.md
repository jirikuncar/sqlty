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
