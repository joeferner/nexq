# Developer entry points. These mirror .github/workflows/ci.yml — `make pre-commit`
# runs the same checks CI does, in the same configuration.

CARGO ?= cargo

# Warnings are errors, matching CI. Note this means `make` and a bare `cargo build`
# use different flags, so switching between them triggers a recompile. Override with
# e.g. `RUSTFLAGS= make build` if that gets in the way.
RUSTFLAGS ?= -D warnings
RUSTDOCFLAGS ?= -D warnings
export RUSTFLAGS
export RUSTDOCFLAGS

# Optional backends that can be compiled out of nexq-server.
BACKENDS := postgres sqlite opensearch elasticsearch

.DEFAULT_GOAL := help
.PHONY: help build test fmt fmt-check clippy doc slim-builds pre-commit clean

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-13s\033[0m %s\n", $$1, $$2}'

build: ## Build every crate, including tests and binaries
	$(CARGO) build --workspace --all-targets --locked

test: ## Run the test suite
	$(CARGO) test --workspace --all-features --locked

fmt: ## Format the workspace in place
	$(CARGO) fmt --all

fmt-check: ## Check formatting without writing (what CI runs)
	$(CARGO) fmt --all --check

clippy: ## Lint every crate and target
	$(CARGO) clippy --workspace --all-targets --all-features --locked

doc: ## Build the API docs
	$(CARGO) doc --workspace --no-deps --all-features --locked

slim-builds: ## Check nexq-server still builds with each backend compiled out
	$(CARGO) check -p nexq-server --no-default-features --locked
	@for feature in $(BACKENDS); do \
		echo "$(CARGO) check -p nexq-server --features $$feature"; \
		$(CARGO) check -p nexq-server --no-default-features \
			--features $$feature --locked || exit 1; \
	done

pre-commit: fmt-check clippy build test doc slim-builds ## Run every check CI runs
	@echo "pre-commit: all checks passed"

clean: ## Remove build artifacts
	$(CARGO) clean
