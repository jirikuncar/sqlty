# Parameter keys

A parameter used twice is one parameter, however often the query names it.

`schema.sql`:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY);
```

```toml
[tool.sqlty]
dialect = "postgres"
schema = "schema.sql"
```

## A repeated parameter counts once

```python
import psycopg

conn = psycopg.connect("dbname=x")
conn.execute("SELECT id FROM users WHERE id = %(id)s OR id = %(id)s + 1", {"id": 1})
conn.execute("SELECT id FROM users WHERE id = %(id)s OR id = %(id)s + 1", {"id": 1, "id": 2})
# error: [param-count-mismatch] "call is missing parameter 'id'"
conn.execute("SELECT id FROM users WHERE id = %(id)s OR id = %(id)s + 1", {})
# error: [param-count-mismatch] "query has 2 parameters, the call passes 1"
conn.execute("SELECT id FROM users WHERE id = %s OR id = %s", (1,))
conn.execute("SELECT id FROM users WHERE id = %s", {"id": 1})
# error: [param-count-mismatch] "query has 1 parameter, the call passes 2"
conn.execute("SELECT id FROM users WHERE id = %s", (1, 2))
```
