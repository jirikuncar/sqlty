# Configuration is read from pyproject.toml in the project root.
# Run from this directory: sqlty check .

# Schema inference uses the configured schema file
user_query = "--sql\nSELECT id, name, email FROM users"
