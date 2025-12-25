-- Sample schema for the psycopg3 cursor wrapper example
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    age INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    title TEXT NOT NULL,
    content TEXT,
    published BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Insert sample data
INSERT INTO users (id, name, email, age) VALUES
    (1, 'Alice Johnson', 'alice@example.com', 30),
    (2, 'Bob Smith', 'bob@example.com', 25),
    (3, 'Charlie Brown', 'charlie@example.com', 35);

INSERT INTO posts (id, user_id, title, content, published) VALUES
    (1, 1, 'First Post', 'Hello World!', true),
    (2, 1, 'Second Post', 'More content here', false),
    (3, 2, 'Bob''s Post', 'Bob''s first post', true);
