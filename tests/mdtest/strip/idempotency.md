# Strip Idempotency

## Already stripped string is unchanged

```python
q = "--sql\nSELECT 1::INTEGER"
# stripped: q = "--sql\nSELECT 1::INTEGER"
```

## Plain string without SQL wrapper is unchanged

```python
q = "hello world"
# stripped: q = "hello world"
```

## Non-string code is unchanged

```python
x = 42
# stripped: x = 42
```
