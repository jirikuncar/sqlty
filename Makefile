# Discover all example directories automatically (excluding hidden directories)
EXAMPLE_DIRS := $(shell find examples -mindepth 1 -maxdepth 1 -type d ! -name ".*")

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
	@echo ""
	@echo "Testing:"
	@echo "  test              - Run all tests (unit + mdtest + e2e)"
	@echo "  test-unit         - Run unit tests only (no e2e, no mdtest)"
	@echo "  test-mdtest       - Run mdtest suite only"
	@echo "  test-e2e          - Run end-to-end tests only"
	@echo "  test-coverage     - Run pytest with coverage report"
	@echo "  update-mdtest     - Update mdtest assertions with actual values"
	@echo ""
	@echo "Examples:"
	@echo "  clean-examples    - Remove all generated .pyi files in examples/"
	@echo "  generate-examples - Generate stubs for all examples"
	@echo "  verify-examples   - Generate stubs and verify with mypy, ty, and stubtest"
	@echo "  example-registries- List registered functions in all examples"
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
typecheck:
	@echo "Running mypy on src/sqlty..."
	uv run mypy src/sqlty
	@echo "Running mypy on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run mypy $$dir; \
	done
	@echo "mypy checks passed"
	@echo ""
	@echo "Running ty on src/sqlty..."
	uv run ty check src/sqlty

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
	uv run pytest tests/mdtest/ --update-mdtest -v

# --- Examples ---

.PHONY: clean-examples
clean-examples:
	@echo "Removing generated .pyi files in examples/"
	@find examples -name "*.pyi" -type f -delete || true
	@echo "Done"

.PHONY: generate-examples
generate-examples: clean-examples
	@echo "Generating stubs for all examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty $$dir/ --format-command "uv run ruff format --stdin-filename {filename}" || exit 1; \
	done
	@echo "All stubs generated"

.PHONY: verify-examples
verify-examples: generate-examples
	@echo "Verifying examples with mypy..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run mypy $$dir/; \
	done
	@echo "mypy verification passed"
	@echo ""
	@echo "Verifying examples with stubtest..."
	@for pyi in $$(find examples -name "*.pyi" -type f); do \
		dir=$$(dirname $$pyi); \
		modname=$$(basename $${pyi%.pyi}); \
		echo "  -> $$pyi"; \
		(cd $$dir && PYTHONPATH=.:$(ROOT_DIR)/src $(ROOT_DIR)/.venv/bin/python -m mypy.stubtest $$modname \
			--allowlist $(ROOT_DIR)/.stubtest-allowlist \
			--ignore-unused-allowlist \
			--concise \
			--mypy-config-file $(ROOT_DIR)/pyproject.toml) || exit 1; \
	done
	@echo "stubtest verification passed"
	@echo ""
	@echo "Verifying magic codec example..."
	@if [ -d examples/magic_codec ]; then \
		echo "  -> examples/magic_codec/demo.py"; \
		(cd examples/magic_codec && PYTHONPATH=$(ROOT_DIR)/src uv run mypy demo.py) || exit 1; \
	fi
	@echo "magic codec verification passed"

.PHONY: example-registries
example-registries:
	@echo "Listing registered functions in all examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo ""; \
		echo "$$dir"; \
		uv run python -m sqlty.registries $$dir/; \
	done
	@echo ""
	@echo "Done"
