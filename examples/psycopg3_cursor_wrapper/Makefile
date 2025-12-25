.PHONY: help start stop clean run typecheck test install

help:  ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

install:  ## Install dependencies
	uv pip install psycopg[binary]

start:  ## Start PostgreSQL container
	docker compose up -d
	@echo "Waiting for PostgreSQL to be ready..."
	@until docker compose exec -T postgres pg_isready -U testuser -d testdb 2>/dev/null; do \
		sleep 1; \
	done
	@echo "PostgreSQL is ready!"

stop:  ## Stop PostgreSQL container
	docker compose down

clean:  ## Stop PostgreSQL and remove volumes
	docker compose down -v

run: start  ## Run the example application
	uv run python app.py

typecheck:  ## Run type checking with mypy
	uv run mypy app.py test_types.py

typecheck-pyright:  ## Run type checking with pyright
	uv run pyright app.py test_types.py

test: start typecheck  ## Run type checks (requires PostgreSQL)
	@echo "Type checking passed!"

all: install start run typecheck  ## Install, start, run, and typecheck
