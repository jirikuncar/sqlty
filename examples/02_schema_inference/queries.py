from sqlty import SQL

# Schema-based inference: no ::TYPE casts needed.
# Run: sqlty rewrite examples/02_schema_inference/  (pyproject.toml sets schema = "schema.sql")

# Types inferred from schema.sql
user_query = SQL[tuple[int, str, str]]("--sql\nSELECT id, name, email FROM users")

orders = SQL[tuple[int, int, float, str]]("--sql\nSELECT id, user_id, total, status FROM orders")

# Explicit casts still work and override schema inference
mixed_query = SQL[tuple[int, str]]("--sql\nSELECT id, name::VARCHAR FROM users")
