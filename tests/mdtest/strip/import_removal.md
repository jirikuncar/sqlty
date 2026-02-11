# Strip Import Removal

## SQL import is removed when no SQL wrappers remain

```python
from sqlty import SQL
q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Datetime import is removed when no datetime types remain

```python
import datetime
from sqlty import SQL
q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM events")
# stripped: q = "--sql\nSELECT created_at::TIMESTAMP FROM events"
```

## UUID import is removed when no uuid types remain

```python
import uuid
from sqlty import SQL
q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID FROM users")
# stripped: q = "--sql\nSELECT user_id::UUID FROM users"
```
