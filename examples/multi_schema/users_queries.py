from sqlty import SQLRegistry


class UsersQueries(SQLRegistry):
    __schema_path__ = "users.sql"


sql = UsersQueries.sql

user_query = sql("SELECT id, name FROM users")
