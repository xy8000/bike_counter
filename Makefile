# Convenience wrappers around common development and CI commands for the
# monorepo (backend/ = Rust crate, frontend/ = React app).
#   make help      -> list all targets
#   make run       -> boot the whole stack (PostgreSQL + backend + frontend) via docker compose
#   make check     -> backend formatting + lint + frontend Prettier check + security-audit gate (scripts/fmt-test.sh + scripts/audit.sh)
#   make fmt       -> rustfmt (backend) + Prettier (frontend)
#   make frontend-fmt -> Prettier-write the frontend
#   make frontend-fmt-check -> Prettier-check the frontend (CI gate)
#   make audit     -> backend dependency security audit (scripts/audit.sh)
#   make test-unit -> frontend unit tests (Vitest; pure-logic modules, no DOM)
#   make test-unit-coverage -> frontend unit tests + v8 coverage (Codecov `frontend` flag)
#   make test-e2e  -> end-to-end docker-compose smoke test (scripts/docker-compose-test.sh)
#   make test-playwright -> Playwright browser e2e tests against the real stack (scripts/e2e-playwright.sh)
#   make playwright-install -> install the Playwright Chromium browser (once)
#   make coverage  -> backend line-coverage gate, overall >= 80% and core >= 95% (scripts/coverage.sh)
#   make coverage-open -> open the HTML coverage report in a browser

.PHONY: help build tiles tiles-update run down logs fmt frontend-fmt frontend-fmt-check check audit test test-rest test-unit test-unit-coverage test-e2e test-playwright playwright-install test-all coverage coverage-open clean frontend-build

help: ## Show available targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build: ## Build the backend (debug)
	cargo build --manifest-path backend/Cargo.toml --quiet

tiles: ## Build tiles/map.pmtiles (worldwide backdrop + Germany detail) via the backend's `tiles` subcommand (reuses go-pmtiles)
	docker compose run --rm --no-deps backend tiles

tiles-update: ## Rebuild tiles/map.pmtiles from a fresh extract (drops the cached file, re-runs the tiles build)
	rm -f tiles/map.pmtiles
	docker compose run --rm --no-deps backend tiles

run: ## Boot the full docker-compose stack (PostgreSQL + backend + frontend) in the foreground
	docker compose up --build

down: ## Stop and remove the docker-compose stack (keeps the database volume)
	docker compose down

logs: ## Follow the logs of all services
	docker compose logs -f

fmt: ## Apply rustfmt to the backend and Prettier to the frontend
	cargo fmt --manifest-path backend/Cargo.toml --quiet
	npm run format --prefix frontend

frontend-fmt: ## Apply Prettier formatting to the frontend
	npm run format --prefix frontend

frontend-fmt-check: ## Check frontend formatting with Prettier (no writes)
	npm run format:check --prefix frontend

check: ## CI gate: rustfmt --check + clippy -D warnings + frontend Prettier check + cargo audit (scripts/fmt-test.sh + scripts/audit.sh)
	./scripts/fmt-test.sh
	npm run format:check --prefix frontend
	./scripts/audit.sh

audit: ## CI gate: cargo audit, fails on any advisory (scripts/audit.sh)
	./scripts/audit.sh

test: ## Run all backend tests (repository tests spin up a Postgres test container via Docker)
	cargo test --manifest-path backend/Cargo.toml --quiet

test-rest: ## Run only the REST endpoint tests (in-memory mocks, no Docker required)
	cargo test --manifest-path backend/Cargo.toml --quiet adapter::driving::rest::tests

test-unit: ## Frontend unit tests with Vitest (pure-logic modules, no DOM)
	npm run test:unit --prefix frontend

test-unit-coverage: ## Frontend unit tests + v8 coverage report (uploaded to Codecov as the `frontend` flag)
	npm run test:unit:coverage --prefix frontend

test-e2e: ## End-to-end smoke test against the real docker-compose stack (requires Docker)
	./scripts/docker-compose-test.sh

test-playwright: ## Playwright browser e2e (all seven data sources) against the real stack seeded from a SQL fixture — jobs disabled, no provider import (scripts/e2e-playwright.sh)
	./scripts/e2e-playwright.sh

playwright-install: ## Install the Playwright Chromium browser into the frontend node_modules (once)
	npm exec --prefix frontend -- playwright install chromium

test-all: check test ## Formatting/lint gate, then the full test suite

coverage: ## Coverage gate (production lines only): overall >= COVERAGE_THRESHOLD (default 80%) and core >= CORE_COVERAGE_THRESHOLD (default 95%) via cargo-llvm-cov (scripts/coverage.sh)
	./scripts/coverage.sh

coverage-open: coverage ## Open the HTML coverage report in a browser
	@(command -v xdg-open >/dev/null 2>&1 && xdg-open backend/target/coverage/html/index.html) || \
	 (command -v open >/dev/null 2>&1 && open backend/target/coverage/html/index.html) || \
	 echo "No browser opener found; open backend/target/coverage/html/index.html manually"

clean: ## Remove backend build artifacts
	cargo clean --manifest-path backend/Cargo.toml

frontend-build: ## Build the React frontend (production bundle into frontend/dist)
	npm run build --prefix frontend
