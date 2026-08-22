#!/usr/bin/env bash
# End-to-end test against the real Docker Compose stack.
#
# Boots PostgreSQL + the application, waits for readiness, then asserts:
#   * GET /api/v1/jobs and GET /api/v1/data-sources return 200
#   * The scheduler recorded a FINISHED data_source_update job (it runs at
#     startup because the job has never succeeded) exposing lifetime_until
#   * The database schema is correct:
#       - jobs.lifetime_until is TIMESTAMPTZ NOT NULL (absolute deadline)
#       - data_sources.last_updated_at exists as TIMESTAMPTZ
#
# A temporary config.toml is created in the project root (the app mounts
# ./config.toml). Any pre-existing config.toml is backed up and restored.
#
# Requirements: Docker, Docker Compose v2 (`docker compose`), curl.
# Override the app endpoint with APP_URL (default http://localhost:8080).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"
CONFIG_FILE="${PROJECT_ROOT}/config.toml"
CONFIG_BACKUP="$(mktemp)"

APP_URL="${APP_URL:-http://localhost:8080}"
DB_SERVICE="db"
DB_USER="${DB_USER:-postgres}"
DB_NAME="${DB_NAME:-bike_counter}"

HAD_CONFIG=0
PASS=0

cleanup() {
  echo "--- Tearing down the Docker Compose stack"
  docker compose -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
  if [ "${HAD_CONFIG}" -eq 1 ]; then
    mv "${CONFIG_BACKUP}" "${CONFIG_FILE}"
    echo "--- Restored original ${CONFIG_FILE}"
  else
    rm -f "${CONFIG_FILE}"
    echo "--- Removed temporary ${CONFIG_FILE}"
  fi
  rm -f "${CONFIG_BACKUP}"
  if [ "${PASS}" -eq 1 ]; then
    echo "docker-compose-test: OK"
  else
    echo "docker-compose-test: FAILED"
  fi
}
trap cleanup EXIT

# Back up an existing config.toml so it can be restored afterwards.
if [ -f "${CONFIG_FILE}" ]; then
  cp "${CONFIG_FILE}" "${CONFIG_BACKUP}"
  HAD_CONFIG=1
fi

# Minimal test config. The database host is the compose service name `db`.
cat > "${CONFIG_FILE}" <<EOF
database_url="postgres://db:5432"
database_user="${DB_USER}"
database_password="postgres"
database_name="${DB_NAME}"

# Data-source update job settings (cron default is hourly).
data_source_update_cron="0 0 * * * *"
data_source_update_max_lifetime_seconds=3600
EOF

echo "--- Clearing any leftover containers from a previous run"
docker compose -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true

echo "--- Building and starting the stack (this builds the release binary)"
docker compose -f "${COMPOSE_FILE}" up -d --build

echo "--- Waiting for ${APP_URL}/health/ready"
READY=0
for _ in $(seq 1 60); do
  if curl --fail --silent "${APP_URL}/health/ready" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 2
done
if [ "${READY}" -ne 1 ]; then
  echo "ERROR: app did not become ready at ${APP_URL}/health/ready" >&2
  docker compose -f "${COMPOSE_FILE}" logs app 2>/dev/null | tail -n 40 || true
  exit 1
fi
echo "App is ready."

assert_status() {
  local expected="$1"
  local uri="$2"
  local code
  code="$(curl --silent --output /dev/null --write-out '%{http_code}' "${APP_URL}${uri}")"
  if [ "${code}" != "${expected}" ]; then
    echo "ERROR: GET ${uri} -> ${code} (expected ${expected})" >&2
    exit 1
  fi
  echo "GET ${uri} -> ${code} (expected ${expected})"
}

assert_status 200 /api/v1/jobs
assert_status 200 /api/v1/data-sources

# The scheduler runs the data-source update at startup (it has never succeeded),
# so a data_source_update job with a lifetime_until deadline must exist.
JOBS_JSON="$(curl --silent "${APP_URL}/api/v1/jobs")"
if ! echo "${JOBS_JSON}" | grep -q "data_source_update"; then
  echo "ERROR: expected at least one data_source_update job in /api/v1/jobs" >&2
  echo "${JOBS_JSON}" >&2
  exit 1
fi
if ! echo "${JOBS_JSON}" | grep -q "lifetime_until"; then
  echo "ERROR: expected jobs to expose lifetime_until" >&2
  echo "${JOBS_JSON}" >&2
  exit 1
fi
echo "Verified data_source_update job with lifetime_until in /api/v1/jobs"

psql_query() {
  docker compose -f "${COMPOSE_FILE}" exec -T "${DB_SERVICE}" \
    psql -U "${DB_USER}" -d "${DB_NAME}" -tAc "$1"
}

# jobs.lifetime_until must be a NOT NULL TIMESTAMPTZ column (absolute deadline).
LIFETIME_TYPE="$(
  psql_query "SELECT data_type || '|' || is_nullable FROM information_schema.columns WHERE table_name='jobs' AND column_name='lifetime_until'"
)"
if [ "${LIFETIME_TYPE}" != "timestamp with time zone|NO" ]; then
  echo "ERROR: jobs.lifetime_until is not TIMESTAMPTZ NOT NULL (got '${LIFETIME_TYPE}')" >&2
  exit 1
fi
echo "Verified jobs.lifetime_until is TIMESTAMPTZ NOT NULL"

# data_sources.last_updated_at must exist as TIMESTAMPTZ (nullable, DB-only).
LAST_UPDATED_TYPE="$(
  psql_query "SELECT data_type FROM information_schema.columns WHERE table_name='data_sources' AND column_name='last_updated_at'"
)"
if [ "${LAST_UPDATED_TYPE}" != "timestamp with time zone" ]; then
  echo "ERROR: data_sources.last_updated_at is missing or not TIMESTAMPTZ (got '${LAST_UPDATED_TYPE}')" >&2
  exit 1
fi
echo "Verified data_sources.last_updated_at is TIMESTAMPTZ"

PASS=1
