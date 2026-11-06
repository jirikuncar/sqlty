# sqruff

```toml
[tool.sqlty.rules]
sqruff = "warn"
stale-rewrite = "ignore"
```

## Escapes - the underline is on the source

```python
# error[49:50]: [sqruff] "AL05: Alias 'U' is never used in SELECT statement."
q = "--sql\nSELECT 'a\\b' AS s, name FROM users u"
```

## Placeholders - a finding on one is dropped

```python
# error:
q = "--sql\nSELECT a FROM t JOIN u ON t.id = u.id WHERE x = %(x)s AND y = %s"
```

## Layout - left to sqlty format

```python
# error:
q = """--sql
select a,b from t where x=1"""
```

## Suppression - on the line the query starts

```python
# error:
q = "--sql\nSELECT name FROM users GROUP BY 1 ORDER BY name"  # sqlty: ignore[sqruff]
```

## Off by default

```toml
[tool.sqlty.rules]
sqruff = "ignore"
```

```python
# error:
q = "--sql\nSELECT name FROM users GROUP BY 1 ORDER BY name"
```
