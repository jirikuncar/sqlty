#!/usr/bin/env bash
# Step 1: three tables and the config that tells sqlty about them.
source "$(dirname "$0")/lib.sh"

cat > schema.sql <<'SQL'
CREATE TABLE ducks (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    colour TEXT NOT NULL,
    hourly_rate REAL NOT NULL
);

CREATE TABLE developers (
    id INTEGER PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE
);

CREATE TABLE consultations (
    id INTEGER PRIMARY KEY,
    duck_id INTEGER NOT NULL REFERENCES ducks (id),
    developer_id INTEGER NOT NULL REFERENCES developers (id),
    bug TEXT NOT NULL,
    solved BOOLEAN NOT NULL DEFAULT FALSE
);
SQL

cat > seed.sql <<'SQL'
INSERT INTO ducks (name, colour, hourly_rate) VALUES
    ('Sir Quacksalot', 'yellow', 99.0),
    ('Admiral Squeaks', 'navy', 42.0),
    ('Segfault Sam', 'black', 0.0);

INSERT INTO developers (handle) VALUES ('you'), ('bob');
SQL

cat >> pyproject.toml <<'TOML'

[tool.sqlty]
schema = "schema.sql"
dialect = "sqlite"
TOML

commit "Step 1: ducks, developers, consultations"
