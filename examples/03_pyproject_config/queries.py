from sqlty import SQL

# Configuration is read from pyproject.toml in the project root.
# Run from this directory: sqlty check .

# Schema inference uses the configured schema file
user_query = SQL[tuple[str, str, str]]("--sql\nSELECT id, name, email FROM users")
