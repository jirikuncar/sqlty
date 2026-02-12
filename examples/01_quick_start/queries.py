# Write SQL queries with --sql prefix and explicit type casts.
# Then run: sqlty rewrite examples/01_quick_start/

user_query = "--sql\nSELECT id::INTEGER, name::TEXT FROM users"

count_query = "--sql\nSELECT COUNT(*)::INTEGER FROM users"

email_query = "--sql\nSELECT id::INTEGER, email::TEXT, active::BOOLEAN FROM users WHERE active = true"
