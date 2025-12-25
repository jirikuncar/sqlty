#!/bin/bash

# Quick start script for the psycopg3 cursor wrapper example

set -e

echo "🚀 Starting psycopg3 cursor wrapper example..."
echo ""

# Check if docker compose is available
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed. Please install Docker first."
    exit 1
fi

# Start PostgreSQL
echo "📦 Starting PostgreSQL container..."
docker compose up -d

# Wait for PostgreSQL to be ready
echo "⏳ Waiting for PostgreSQL to be ready..."
max_attempts=30
attempt=0
until docker compose exec -T postgres pg_isready -U testuser -d testdb &> /dev/null; do
    attempt=$((attempt + 1))
    if [ $attempt -eq $max_attempts ]; then
        echo "❌ PostgreSQL failed to start after $max_attempts attempts"
        docker compose logs postgres
        exit 1
    fi
    sleep 1
done

echo "✅ PostgreSQL is ready!"
echo ""

# Install dependencies if needed
if ! python -c "import psycopg" 2> /dev/null; then
    echo "📦 Installing dependencies..."
    uv pip install psycopg[binary] || {
        echo "❌ Failed to install dependencies"
        exit 1
    }
fi

# Run the application
echo "🎯 Running the application..."
echo ""
uv run python app.py

echo ""
echo "✅ Example completed successfully!"
echo ""
echo "To stop PostgreSQL: docker compose down"
echo "To remove data: docker compose down -v"
