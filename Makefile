.PHONY: help setup build release run test fmt fmt-check lint check clean

CARGO ?= cargo

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

setup: ## Install the Rust toolchain (via rustup) and fetch dependencies
	@command -v rustup >/dev/null || { echo "rustup not found: install from https://rustup.rs"; exit 1; }
	rustup show active-toolchain || rustup toolchain install
	rustup component add rustfmt clippy
	$(CARGO) fetch

build: ## Build a debug binary
	$(CARGO) build

release: ## Build an optimized release binary (target/release/ghui)
	$(CARGO) build --release

run: ## Run the TUI in debug mode
	$(CARGO) run

test: ## Run all tests
	$(CARGO) test --all-targets

fmt: ## Format the code
	$(CARGO) fmt --all

fmt-check: ## Check formatting without modifying files
	$(CARGO) fmt --all -- --check

lint: ## Run clippy, treating warnings as errors
	$(CARGO) clippy --all-targets -- -D warnings

check: fmt-check lint test ## Run everything CI runs

clean: ## Remove build artifacts
	$(CARGO) clean
