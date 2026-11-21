CREATE TABLE users (id INT NOT NULL, name TEXT);
CREATE TABLE orders (id INT NOT NULL, user_id INT REFERENCES users (id));
CREATE VIEW active_users AS SELECT id FROM users;
