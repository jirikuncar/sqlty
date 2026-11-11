import decimal
from sqlty import SQL

# Schema-based inference: no ::TYPE casts needed.
# Run from this directory: sqlty rewrite .  (schema.sql is set in pyproject.toml)

# Types inferred from schema.sql
user_query = SQL[tuple[int, str, str | None]]("--sql\nSELECT id, name, email FROM users")

order_query = SQL[tuple[int, int, decimal.Decimal, str | None]](
    "--sql\nSELECT id, user_id, total, status FROM orders"
)

# Explicit casts still work and override schema inference
mixed_query = SQL[tuple[int, str]]("--sql\nSELECT id, name::VARCHAR FROM users")
