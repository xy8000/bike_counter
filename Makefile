# Convenience wrappers around common development and CI commands for the
# monorepo (backend/ = Rust crate, frontend/ = React app).
#   make help      -> list all targets
#   make run       -> boot the whole stack (PostgreSQL + backend + frontend) via docker compose
#   make check     -> backend formatting + lint gate (scripts/fmt-test.sh)
#   make test-e2e  -> end-to-end docker-compose smoke test (scripts/docker-compose-test.sh)
#   make test-playwright -> Playwright browser e2e tests against the real stack (scripts/e2e-playwright.sh)
#   make playwright-install -> install the Playwright Chromium browser (once)
#   make coverage  -> backend line-coverage gate, overall >= 80% and core >= 95% (scripts/coverage.sh)
#   make coverage-open -> open the HTML coverage report in a browser

.PHONY: help build run down logs fmt check test test-rest test-e2e test-playwright playwright-install test-all coverage coverage-open clean frontend-build

help: ## Show available targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Build the backend (debug)
	cd backend && cargo build

run: ## Boot the full docker-compose stack (PostgreSQL + backend + frontend) in the foreground
	docker compose up --build

down: ## Stop and remove the docker-compose stack (keeps the database volume)
	docker compose down

logs: ## Follow the logs of all services
	docker compose logs -f

fmt: ## Apply rustfmt formatting to the backend
	cd backend && cargo fmt

check: ## CI gate: rustfmt --check + clippy -D warnings (scripts/fmt-test.sh)
	./scripts/fmt-test.sh

test: ## Run all backend tests (repository tests spin up a Postgres test container via Docker)
	cd backend && cargo test

test-rest: ## Run only the REST endpoint tests (in-memory mocks, no Docker required)
	cd backend && cargo test adapter::driving::rest::tests

test-e2e: ## End-to-end smoke test against the real docker-compose stack (requires Docker)
	./scripts/docker-compose-test.sh

test-playwright: ## Playwright browser e2e tests against the real stack with a real Münster import (requires Docker + GitHub access; scripts/e2e-playwright.sh)
	./scripts/e2e-playwright.sh

playwright-install: ## Install the Playwright Chromium browser into the frontend node_modules (once)
	cd frontend && npx playwright install chromium

test-all: check test ## Formatting/lint gate, then the full test suite

coverage: ## Coverage gate (production lines only): overall >= COVERAGE_THRESHOLD (default 80%) and core >= CORE_COVERAGE_THRESHOLD (default 95%) via cargo-llvm-cov (scripts/coverage.sh)
	./scripts/coverage.sh

coverage-open: coverage ## Open the HTML coverage report in a browser
	@(command -v xdg-open >/dev/null 2>&1 && xdg-open backend/target/coverage/html/index.html) || \
	 (command -v open >/dev/null 2>&1 && open backend/target/coverage/html/index.html) || \
	 echo "No browser opener found; open backend/target/coverage/html/index.html manually"

clean: ## Remove backend build artifacts
	cd backend && cargo clean

frontend-build: ## Build the React frontend (production bundle into frontend/dist)
	cd frontend && npm run build
