# Corpus: MySQL

MySQL follows the same NULL rules for `CASE`: it is NULL only when a branch can be or no
branch matches without an `ELSE`.

```toml
[tool.sqlty]
dialect = "mysql"
schema = "schema.sql"
```

## CASE over NOT NULL integer columns

Both branches are NOT NULL `INT` columns and there is an `ELSE`, so the result is a non-null
integer.

`schema.sql`:

```sql
CREATE TABLE scores (
    id INT UNSIGNED NOT NULL AUTO_INCREMENT,
    val1 INT NOT NULL,
    val2 INT NOT NULL,
    PRIMARY KEY (id)
);
```

```sql
SELECT id, CASE WHEN val1 = 100 THEN val1 ELSE val2 END AS final_val FROM scores
-- inferred: tuple[int, int]
```
