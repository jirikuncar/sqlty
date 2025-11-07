.PHONY: help run show-stub clean

help:
	@echo "Runtime Auto-Generation Example"
	@echo ""
	@echo "Available targets:"
	@echo "  run        - Run the demo with SQLTY_AUTO_GENERATE=1"
	@echo "  show-stub  - Display the generated stub file"
	@echo "  clean      - Remove generated files"

run:
	@echo "Running demo with auto-generation enabled..."
	pushd ../../ && SQLTY_AUTO_GENERATE=1 uv run python examples/runtime_autogen/demo.py && popd
	@echo ""
	@echo "Stub file generated! Use 'make show-stub' to view it."

show-stub:
	@if [ -f queries.pyi ]; then \
		echo "Generated stub file (queries.pyi):"; \
		echo "================================"; \
		cat queries.pyi; \
	else \
		echo "No stub file found. Run 'make run' first."; \
	fi

clean:
	@echo "Cleaning generated files..."
	rm -f *.pyi *.pyi.lock
	@echo "Done!"
