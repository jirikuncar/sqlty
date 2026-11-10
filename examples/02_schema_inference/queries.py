import decimal
from sqlty import SQL
from sqlty import NoParams

# Schema-based inference: no ::TYPE casts needed.
# Run from this directory: sqlty rewrite .  (schema.sql is set in pyproject.toml)

# Types inferred from schema.sql
user_query = SQL[tuple[int, str, str | None], NoParams]("--sql\nSELECT id, name, email FROM users")

order_query = SQL[tuple[int, int, decimal.Decimal, str | None], NoParams](
    "--sql\nSELECT id, user_id, total, status FROM orders"
)

# Explicit casts still work and override schema inference
mixed_query = SQL[tuple[int, str], NoParams]("--sql\nSELECT id, name::VARCHAR FROM users")
