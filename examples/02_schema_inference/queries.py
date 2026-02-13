from sqlty import SQL

# Schema-based inference: no ::TYPE casts needed.
# Run: sqlty rewrite --schema examples/02_schema_inference/schema.sql examples/02_schema_inference/

# Types inferred from schema.sql
user_query = SQL[tuple[str, str, str]]("--sql\nSELECT id, name, email FROM users")

order_query = SQL[tuple[str, str, str, str]]("--sql\nSELECT id, user_id, total, status FROM orders")

# Explicit casts still work and override schema inference
mixed_query = SQL[tuple[str, str]]("--sql\nSELECT id, name::VARCHAR FROM users")
