-- Example schema for SQLTy
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

-- You can add more tables as needed
CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    content TEXT NOT NULL
);
