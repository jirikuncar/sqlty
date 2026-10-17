# Discover all example directories automatically (excluding hidden and magic_codec directories)
# magic_codec uses "# coding: sqlty" which type checkers cannot decode
EXAMPLE_DIRS := $(shell find examples -mindepth 1 -maxdepth 1 -type d ! -name ".*" ! -name "magic_codec")

# Zensical is pinned only here (CI runs `make docs`), outside the project dependency groups
ZENSICAL := uvx --python 3.13 zensical@0.0.65

# Get the absolute path to the repository root
ROOT_DIR := $(shell pwd)

.PHONY: help
help:
	@echo "Available targets:"
	@echo ""
	@echo "Development:"
	@echo "  install           - Install/sync dependencies with uv"
	@echo "  hooks-install     - Install the prek git hook"
	@echo "  hooks             - Run every prek hook on all files"
	@echo "  lint              - Run ruff linter with auto-fix"
	@echo "  format            - Run ruff formatter"
	@echo "  typecheck         - Run mypy and ty type checks on src and examples"
	@echo "  typecheck-src     - Run mypy and ty on src/sqlty"
	@echo "  typecheck-examples- Run mypy and ty on examples (except magic_codec)"
	@echo ""
	@echo "Testing:"
	@echo "  test              - Run all tests (unit + mdtest + e2e)"
	@echo "  test-unit         - Run unit tests only (no e2e, no mdtest)"
	@echo "  test-mdtest       - Run mdtest suite and docs tests"
	@echo "  test-e2e          - Run end-to-end tests only"
	@echo "  test-coverage     - Run pytest with coverage report"
	@echo "  update-mdtest     - Update mdtest assertions with actual values"
	@echo "  bench             - Run sqlty.core benchmarks"
	@echo "  bench-project     - Benchmark a synthetic 10k-file project"
	@echo "  bench-ci          - CI performance budget check against benchmarks/baseline.json"
	@echo "  bench-baseline    - Re-record the benchmark baseline"
	@echo "  bench-rust        - Criterion benchmarks of the Rust engine (rust/crates/sqlty-bench)"
	@echo ""
	@echo "Docs:"
	@echo "  docs              - Build the docs site, strict via zensical.toml (output: site/)"
	@echo "  docs-serve        - Serve the docs locally with live reload"
	@echo "  generate-docs     - Regenerate the tables in docs/reference/{configuration,cli}.md"
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

.PHONY: hooks-install
hooks-install:
	uv run prek install

.PHONY: hooks
hooks:
	uv run prek run --all-files

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
	uv run pytest tests/mdtest/ docs/ src/sqlty/rules/ README.md -v

.PHONY: test-e2e
test-e2e:
	uv run pytest tests/e2e/ -v

.PHONY: test-coverage
test-coverage:
	uv run pytest tests/ -v --cov=sqlty --cov-report=term-missing

.PHONY: update-mdtest
update-mdtest:
	uv run pytest tests/mdtest/ docs/ src/sqlty/rules/ README.md tests/test_docs.py --update-mdtest -v

# --- Benchmarks ---

.PHONY: bench
bench:
	uv run python benchmarks/bench_core.py

.PHONY: bench-project
bench-project:  ## Full-size synthetic project (10k files); needs a release build
	uv run python benchmarks/bench_project.py --size full

.PHONY: bench-ci
bench-ci:  ## CI-size project; exits 1 when over the budget in benchmarks/baseline.json
	uv run python benchmarks/bench_project.py --size ci --repeats 7 --compare --out bench-result.json

.PHONY: bench-baseline
bench-baseline:  ## Re-record the CI-size baseline for this platform
	uv run python benchmarks/bench_project.py --size ci --repeats 7 --update-baseline

.PHONY: bench-rust
bench-rust:  ## Criterion: catalog load, analyzer, check/rewrite_source; extra args via ARGS=
	cargo bench --manifest-path rust/Cargo.toml -p sqlty-bench -- $(ARGS)

# --- Docs ---

.PHONY: docs
docs:
	$(ZENSICAL) build --clean

.PHONY: lint-docs
lint-docs:
	uv run prek run rumdl --all-files
	uv run prek run vale --all-files

.PHONY: generate-docs
generate-docs:
	uv run python scripts/generate_docs.py

.PHONY: docs-serve
docs-serve:
	$(ZENSICAL) serve

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
