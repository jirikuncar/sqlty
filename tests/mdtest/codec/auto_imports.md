# Magic Codec Auto Imports

## Datetime import added for TIMESTAMP

```python
q = "--sql\nSELECT created_at::TIMESTAMP"
# decoded: q = SQL[tuple[datetime.datetime]]("--sql\nSELECT created_at::TIMESTAMP")
```

## UUID import added for UUID type

```python
q = "--sql\nSELECT user_id::UUID"
# decoded: q = SQL[tuple[uuid.UUID]]("--sql\nSELECT user_id::UUID")
```
