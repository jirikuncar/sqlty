# /// script
# requires-python = ">=3.14"
# dependencies = [
#     "psycopg[binary]",
# ]
# ///
"""Example with multiple SQL queries to test stub generation."""

from collections.abc import Iterator

from psycopg import Connection
from psycopg.abc import Params

from sqlty.drivers.psycopg import execute

from .queries import sql

# Multiple queries with different return types
get_user_query = sql(
    """
    SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id
    """
)

get_posts_query = sql(
    """
    SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id
    """
)

count_query = sql(
    """
    SELECT COUNT(*)::INTEGER FROM users
    """
)


def main() -> None:
    conn = Connection.connect("dbname=test user=postgres password=secret host=localhost port=5432")

    # Use first query
    params: Params = {"id": 1}
    users: Iterator[tuple[int, str]] = execute(conn, get_user_query, params)
    for user in users:
        print(f"User ID: {user[0]}, Name: {user[1]}")

    # Use second query
    post_params: Params = {"user_id": 1}
    posts: Iterator[tuple[int, str, str]] = execute(conn, get_posts_query, post_params)
    for post in posts:
        print(f"Post ID: {post[0]}, Title: {post[1]}, Content: {post[2]}")

    # Use third query
    counts: Iterator[tuple[int]] = execute(conn, count_query, {})
    for count in counts:
        print(f"Total users: {count[0]}")

    conn.close()


if __name__ == "__main__":
    main()
