# Convenience wrappers around common development and CI commands.
#   make help      -> list all targets
#   make check     -> formatting + lint gate (scripts/fmt-test.sh)
#   make test-e2e  -> end-to-end docker-compose smoke test (scripts/docker-compose-test.sh)

.PHONY: help build run down logs fmt check test test-rest test-e2e test-all clean

help: ## Show available targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Build the project (debug)
	cargo build

run: ## Boot the full docker-compose stack (PostgreSQL + app) in the foreground
	docker compose up --build

down: ## Stop and remove the docker-compose stack (keeps the database volume)
	docker compose down

logs: ## Follow the application logs
	docker compose logs -f app

fmt: ## Apply rustfmt formatting
	cargo fmt

check: ## CI gate: rustfmt --check + clippy -D warnings (scripts/fmt-test.sh)
	./scripts/fmt-test.sh

test: ## Run all tests (repository tests spin up a Postgres test container via Docker)
	cargo test

test-rest: ## Run only the REST endpoint tests (in-memory mocks, no Docker required)
	cargo test adapter::driving::rest::tests

test-e2e: ## End-to-end smoke test against the real docker-compose stack (requires Docker)
	./scripts/docker-compose-test.sh

test-all: check test ## Formatting/lint gate, then the full test suite

clean: ## Remove build artifacts
	cargo clean
