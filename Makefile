# Discover all example directories automatically (excluding hidden and magic_codec directories)
# magic_codec uses "# coding: sqlty" which type checkers cannot decode
EXAMPLE_DIRS := $(shell find examples -mindepth 1 -maxdepth 1 -type d ! -name ".*" ! -name "magic_codec")

# Get the absolute path to the repository root
ROOT_DIR := $(shell pwd)

.PHONY: help
help:
	@echo "Available targets:"
	@echo ""
	@echo "Development:"
	@echo "  install           - Install/sync dependencies with uv"
	@echo "  pre-commit-install- Install pre-commit hooks"
	@echo "  pre-commit        - Run pre-commit on all files"
	@echo "  lint              - Run ruff linter with auto-fix"
	@echo "  format            - Run ruff formatter"
	@echo "  typecheck         - Run mypy and ty type checks on src and examples"
	@echo "  typecheck-src     - Run mypy and ty on src/sqlty"
	@echo "  typecheck-examples- Run mypy and ty on examples (except magic_codec)"
	@echo ""
	@echo "Testing:"
	@echo "  test              - Run all tests (unit + mdtest + e2e)"
	@echo "  test-unit         - Run unit tests only (no e2e, no mdtest)"
	@echo "  test-mdtest       - Run mdtest suite only"
	@echo "  test-e2e          - Run end-to-end tests only"
	@echo "  test-coverage     - Run pytest with coverage report"
	@echo "  update-mdtest     - Update mdtest assertions with actual values"
	@echo "  bench             - Run Python vs Rust benchmarks"
	@echo ""
	@echo "Examples:"
	@echo "  check-examples    - Run sqlty check on all examples"
	@echo "  rewrite-examples  - Run sqlty rewrite on all examples"
	@echo "  strip-examples    - Run sqlty strip on all examples"
	@echo ""
	@echo "Example directories found: $(EXAMPLE_DIRS)"

.PHONY: install
install:
	uv sync --all-groups

.PHONY: pre-commit-install
pre-commit-install:
	uv run pre-commit install

.PHONY: pre-commit
pre-commit:
	uv run pre-commit run --all-files

.PHONY: lint
lint:
	uv run ruff check --fix .

.PHONY: format
format:
	uv run ruff format .

.PHONY: typecheck
typecheck: typecheck-src typecheck-examples

.PHONY: typecheck-src
typecheck-src:
	uv run mypy src/sqlty
	uv run ty check src/sqlty

.PHONY: typecheck-examples
typecheck-examples:
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run mypy $$dir || exit 1; \
		uv run ty check $$dir || exit 1; \
	done

# --- Testing targets ---

.PHONY: test
test:
	uv run pytest

.PHONY: test-unit
test-unit:
	uv run pytest tests/ --ignore=tests/e2e --ignore=tests/mdtest -q

.PHONY: test-mdtest
test-mdtest:
	uv run pytest tests/mdtest/ -v

.PHONY: test-e2e
test-e2e:
	uv run pytest tests/e2e/ -v

.PHONY: test-coverage
test-coverage:
	uv run pytest tests/ -v --cov=sqlty --cov-report=term-missing

.PHONY: update-mdtest
update-mdtest:
	uv run pytest tests/mdtest/ tests/test_docs.py --update-mdtest -v

# --- Benchmarks ---

.PHONY: bench
bench:
	uv run python benchmarks/bench_core.py

# --- Examples ---

.PHONY: check-examples
check-examples:
	@echo "Running sqlty check on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty check $$dir/ || true; \
	done
	@echo "Done"

.PHONY: rewrite-examples
rewrite-examples:
	@echo "Running sqlty rewrite on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty rewrite $$dir/ || true; \
	done
	@echo "Done"

.PHONY: strip-examples
strip-examples:
	@echo "Running sqlty strip on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty strip $$dir/ || true; \
	done
	@echo "Done"
