# Discover all example directories automatically (excluding hidden and magic_codec directories)
# magic_codec uses "# coding: sqlty" which type checkers cannot decode
EXAMPLE_DIRS := $(shell find examples -mindepth 1 -maxdepth 1 -type d ! -name ".*" ! -name "magic_codec")

# Zensical is pinned only here (CI runs `make docs`), outside the project dependency groups
ZENSICAL := uv run --no-sync --with zensical==0.1.0 zensical

.PHONY: help
help:
	@awk -F':.*## ' '/^# --- /{gsub(/# --- | ---/, ""); printf "\n%s:\n", $$0} /^[a-z0-9-]+:.*## /{printf "  %-22s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# --- Development ---

.PHONY: install
install:  ## Install every dependency group
	uv sync --all-groups

.PHONY: hooks-install
hooks-install:  ## Install the prek git hook
	uv run prek install

.PHONY: hooks
hooks:  ## Run every prek hook on all files
	uv run prek run --all-files

.PHONY: typecheck
typecheck:  ## mypy and ty on src and examples (the prek hooks)
	uv run prek run --all-files mypy ty typecheck-examples

.PHONY: typecheck-examples
typecheck-examples:  ## mypy and ty on each example (except magic_codec)
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run mypy $$dir || exit 1; \
		uv run ty check $$dir || exit 1; \
	done

# --- Testing ---

.PHONY: test
test:  ## All tests, then the timing tests
	uv run pytest
	$(MAKE) test-timing

.PHONY: test-timing
test-timing:  ## The tests that measure wall time, alone and in order
	uv run pytest -m timing -n 0 -p no:randomly

.PHONY: test-unit
test-unit:  ## Unit tests only (no e2e, no mdtest)
	uv run pytest tests/ --ignore=tests/e2e --ignore=tests/mdtest -q

.PHONY: test-mdtest
test-mdtest:  ## mdtests and docs tests
	uv run pytest tests/mdtest/ docs/ README.md -v

.PHONY: test-e2e
test-e2e:  ## End-to-end tests only
	uv run pytest tests/e2e/ -v

.PHONY: check-no-exec-harness
check-no-exec-harness:  ## Fail if this branch weakened the no-exec harness (AGENTS.md#no-exec)
	uv run python scripts/check_no_exec_harness.py origin/main

.PHONY: test-coverage
test-coverage:  ## Every Python and Rust test with coverage and its gates; needs a nightly toolchain with llvm-tools, cargo-llvm-cov and cargo-nextest
	scripts/coverage.sh

.PHONY: update-mdtest
update-mdtest:  ## Update mdtest assertions with actual values
	uv run pytest tests/mdtest/ docs/ README.md tests/test_docs.py --update-mdtest -v

# --- Benchmarks ---

.PHONY: bench-project
bench-project:  ## Full-size synthetic project (10k files); needs a release build
	uv run python benchmarks/bench_project.py --size full

.PHONY: bench-ci
bench-ci:  ## CI-size project; exits 1 when over the budget in benchmarks/baseline.json
	uv run python benchmarks/bench_project.py --size ci --repeats 7 --compare --out bench-result.json

.PHONY: bench-ab
bench-ab:  ## Interleaved A/B of `sqlty check`, BASE (default origin/main) vs the working tree; extra args via ARGS=
	uv run python benchmarks/ab.py --base $(or $(BASE),origin/main) $(ARGS)

.PHONY: bench-baseline
bench-baseline:  ## Re-record the CI-size baseline for this platform
	uv run python benchmarks/bench_project.py --size ci --repeats 7 --update-baseline

.PHONY: bench-corpus
bench-corpus:  ## sqlty check on pinned real repositories (clones them); extra args via ARGS=
	uv run python benchmarks/corpus.py $(ARGS)

.PHONY: bench-codspeed
bench-codspeed:  ## The benchmarks CodSpeed measures in CI, timed locally; needs a release build
	uv run --group bench pytest benchmarks/ --codspeed -n 0 -p no:randomly --no-cov

.PHONY: bench-rust
bench-rust:  ## Criterion: catalog load, analyzer, check/rewrite_source, one query; extra args via ARGS=
	cargo bench --manifest-path rust/Cargo.toml -p sqlty-bench -- $(ARGS)

# --- Docs ---

.PHONY: docs
docs: playground  ## Build the docs site, strict via zensical.toml (output: site/)
	$(ZENSICAL) build --clean
	python3 scripts/playground_links.py site
	python3 scripts/structured_data.py site
	python3 scripts/offline.py site

.PHONY: wasm
wasm:  ## The engine as WebAssembly, in the @sqlty/wasm package (needs the wasm32-unknown-unknown target)
	cargo build --manifest-path rust/Cargo.toml --profile wasm --target wasm32-unknown-unknown -p sqlty-wasm
	cp rust/target/wasm32-unknown-unknown/wasm/sqlty_wasm.wasm npm/wasm/

.PHONY: playground
playground: wasm  ## The engine as WebAssembly, and Monaco and the fonts, for the docs playground
	cp npm/wasm/sqlty_wasm.wasm npm/wasm/engine.js docs/assets/playground/
	python3 scripts/generate_playground_examples.py
	python3 scripts/vendor_assets.py

.PHONY: check-playground
check-playground: docs  ## Load the built playground under /sqlty/ in Chromium (needs `playwright install chromium`)
	uvx --python 3.13 --with playwright==1.64.1 python scripts/check_playground.py --site site

.PHONY: lint-docs
lint-docs:  ## rumdl and Vale on the Markdown (the prek hooks)
	uv run prek run --all-files rumdl vale

.PHONY: generate-docs
generate-docs:  ## Regenerate the tables in docs/reference/{configuration,cli,rules}.md
	uv run python scripts/generate_docs.py

.PHONY: generate-stdlib-types
generate-stdlib-types:  ## Regenerate the standard-library class table
	uv run python scripts/generate_stdlib_types.py

.PHONY: generate-duckdb-rules
generate-duckdb-rules:  ## Regenerate the DuckDB function rules from DuckDB itself
	uv run python scripts/generate_duckdb_rules.py

.PHONY: docs-serve
docs-serve: playground  ## Serve the docs locally with live reload
	$(ZENSICAL) serve

# --- Examples ---

.PHONY: check-examples
check-examples:  ## sqlty check on every example
	@echo "Running sqlty check on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty check $$dir/ || true; \
	done
	@echo "Done"

.PHONY: rewrite-examples
rewrite-examples:  ## sqlty rewrite on every example
	@echo "Running sqlty rewrite on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty rewrite $$dir/ || true; \
	done
	@echo "Done"

.PHONY: strip-examples
strip-examples:  ## sqlty strip on every example
	@echo "Running sqlty strip on examples..."
	@for dir in $(EXAMPLE_DIRS); do \
		echo "  -> $$dir"; \
		uv run sqlty strip $$dir/ || true; \
	done
	@echo "Done"
