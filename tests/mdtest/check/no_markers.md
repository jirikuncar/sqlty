# Files without markers produce no diagnostics

`sqlty check` skips the Rust checker for a file that holds no `--sql` / `-- sql`
marker, `SQL[` wrapper call or `UNTYPED`. The result is the same as running it.

```python
x = "SELECT id FROM users"
# error:
```

## A marker anywhere still runs the checker

```python
# error: [stale-rewrite] "Source has --sql strings that aren't wrapped in SQL[T]"
x = "--sql\nSELECT 1::INTEGER AS n"
```
