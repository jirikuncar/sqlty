# Import Injection

## SQL import is added

```python
q = "--sql\nSELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

Whole file after rewrite:

```python
q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

## Datetime import is added

```python
q = "--sql\nSELECT created_at::TIMESTAMP FROM events"
# rewritten: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM events")
```

Whole file after rewrite:

```python
q = "--sql\nSELECT created_at::TIMESTAMP FROM events"
# rewritten-file: import datetime
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP FROM events")
```

## UUID import is added

```python
q = "--sql\nSELECT user_id::UUID FROM users"
# rewritten: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID FROM users")
```

Whole file after rewrite:

```python
q = "--sql\nSELECT user_id::UUID FROM users"
# rewritten-file: import uuid
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID FROM users")
```

## Existing import is not duplicated

```python
from sqlty import SQL
q = "--sql\nSELECT 1::INTEGER"
# rewritten: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```

Whole file after rewrite (no duplicate import):

```python
from sqlty import SQL
q = "--sql\nSELECT 1::INTEGER"
# rewritten-file: from sqlty import SQL
# rewritten-file: q = SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")
```
