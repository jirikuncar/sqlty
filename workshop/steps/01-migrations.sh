#!/usr/bin/env bash
# Step 1: Postgres in Docker, three tables as migrations, a twenty-line migrator.
source "$(dirname "$0")/lib.sh"

cat > compose.yaml <<'YAML'
name: duckdesk

services:
  db:
    image: postgres:17-alpine
    environment:
      POSTGRES_USER: duck
      POSTGRES_PASSWORD: quack
    ports: ["127.0.0.1:5432:5432"]
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "duck"]
      interval: 1s
      retries: 30

networks:
  default:
    name: duckdesk
YAML

mkdir -p migrations
cat > migrations/001_tables.sql <<'SQL'
CREATE TABLE ducks (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    colour TEXT NOT NULL,
    hourly_rate NUMERIC(6, 2) NOT NULL
);

CREATE TABLE developers (
    id SERIAL PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE
);

CREATE TABLE consultations (
    id SERIAL PRIMARY KEY,
    duck_id INTEGER NOT NULL REFERENCES ducks (id),
    developer_id INTEGER NOT NULL REFERENCES developers (id),
    bug TEXT NOT NULL,
    solved BOOLEAN NOT NULL DEFAULT FALSE
);
SQL

cat > migrations/002_ducks.sql <<'SQL'
INSERT INTO ducks (name, colour, hourly_rate) VALUES
    ('Sir Quacksalot', 'yellow', 99.00),
    ('Admiral Squeaks', 'navy', 42.00),
    ('Segfault Sam', 'black', 0.00);

INSERT INTO developers (handle) VALUES ('you'), ('bob');
SQL

cat > migrate.py <<'PY'
"""Apply migrations/*.sql in order, each once: uv run python migrate.py"""

import os
from pathlib import Path

import psycopg
from psycopg import sql
from psycopg.conninfo import conninfo_to_dict

DATABASE_URL = os.environ.get("DATABASE_URL", "postgresql://duck:quack@localhost:5432/duckdesk_dev")
MIGRATIONS = Path(__file__).parent / "migrations"


def migrate(url: str = DATABASE_URL) -> None:
    dbname = str(conninfo_to_dict(url)["dbname"])
    with psycopg.connect(url, dbname="postgres", autocommit=True) as server:
        if not server.execute("SELECT 1 FROM pg_database WHERE datname = %s", (dbname,)).fetchone():
            server.execute(sql.SQL("CREATE DATABASE {}").format(sql.Identifier(dbname)))

    with psycopg.connect(url, autocommit=True) as conn:
        conn.execute("SELECT pg_advisory_lock(42)")
        conn.execute("CREATE TABLE IF NOT EXISTS applied_migrations (name TEXT PRIMARY KEY)")
        applied = {name for (name,) in conn.execute("SELECT name FROM applied_migrations")}
        for path in sorted(MIGRATIONS.glob("*.sql")):
            if path.name not in applied:
                with conn.transaction():
                    conn.execute(path.read_bytes())
                    conn.execute("INSERT INTO applied_migrations VALUES (%s)", (path.name,))
                print(f"applied {path.name}")


if __name__ == "__main__":
    migrate()
PY

cat >> pyproject.toml <<'TOML'

[tool.sqlty]
schema = "migrations/"
TOML

docker compose up -d --wait db
uv run python migrate.py

commit "Step 1: ducks, developers, consultations"
