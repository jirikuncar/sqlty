CREATE TABLE users (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT,
    balance NUMERIC(10, 2) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    tags TEXT[] NOT NULL,
    scores NUMERIC[]
);
