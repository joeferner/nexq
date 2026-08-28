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

# Local config for `make server`. Gitignored, since it holds a real secret.
CONFIG ?= $(CURDIR)/nexq.toml

.DEFAULT_GOAL := help
.PHONY: help build server test fmt fmt-check clippy doc pre-commit clean

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-13s\033[0m %s\n", $$1, $$2}'

build: ## Build every crate, including tests and binaries
	$(CARGO) build --workspace --all-targets --locked

server: ## Run the server against ./nexq.toml, seeding it from the example if absent
	@test -f "$(CONFIG)" || { \
		cp nexq.example.toml "$(CONFIG)"; \
		echo "created $(CONFIG) from nexq.example.toml — change the secret before exposing it"; \
	}
	NEXQ_CONFIG="$(CONFIG)" $(CARGO) run -p nexq-server --locked

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

openapi: ## Regenerate crates/nexq-api-rest/openapi.json from the REST router
	$(CARGO) xtask openapi

openapi-check: ## Fail if the committed openapi.json no longer matches the code
	$(CARGO) xtask openapi-check

acceptance-cli: ## Drive a real server with the real aws CLI (needs the aws CLI installed)
	$(CARGO) xtask acceptance-cli

acceptance-node: ## Same, with the AWS SDK for JavaScript (needs Node.js installed)
	$(CARGO) xtask acceptance-node

acceptance-rest: ## Drive the REST facade with real curl (needs curl installed)
	$(CARGO) xtask acceptance-rest

# What is in here, and what is deliberately not:
#
# `openapi-check` is in, before `test`, so a change to the published contract arrives as
# its own named failure rather than as one test among hundreds. The same comparison also
# runs inside `cargo test`; that duplication is on purpose, since `cargo test` is what
# most people run and the check should not depend on remembering this target.
#
# The acceptance suites are out. Each starts a server and waits out real long-poll
# timeouts, so they take real wall clock apiece. CI runs them as their own jobs, and
# `make acceptance-cli` / `make acceptance-node` / `make acceptance-rest` run them here.
pre-commit: fmt-check clippy build openapi-check test doc ## Run every check CI runs
	@echo "pre-commit: all checks passed"

clean: ## Remove build artifacts
	$(CARGO) clean
