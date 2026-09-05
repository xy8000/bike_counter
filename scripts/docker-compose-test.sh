#!/usr/bin/env bash
# End-to-end test against the real Docker Compose stack.
#
# Boots PostgreSQL + backend + frontend, waits for readiness, then asserts:
#   * GET /api/v1/jobs and GET /api/v1/data-sources return 200
#   * The scheduler recorded a data_source_update job (it runs at startup
#     because the job has never succeeded) exposing instance_id / heartbeat_at
#   * GET /api/bff/stations returns the BFF station list (frontend-facing BFF API)
#   * The frontend page is served
#   * The database schema is correct:
#       - jobs.instance_id / heartbeat_at exist (nullable ownership/liveness)
#       - the job_locks ShedLock table exists
#       - data_sources.imported_until exists as TIMESTAMPTZ
#
# A temporary config.toml is created at the repo root (the backend service
# mounts ./config.toml). Any pre-existing config.toml is backed up and restored.
#
# Requirements: Docker, Docker Compose v2 (`docker compose`), curl.
# Override the endpoints with APP_URL (default http://localhost:8080) and
# FRONTEND_URL (default http://localhost:8081).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"
CONFIG_FILE="${PROJECT_ROOT}/config.toml"
CONFIG_BACKUP="$(mktemp)"

APP_URL="${APP_URL:-http://localhost:8080}"
FRONTEND_URL="${FRONTEND_URL:-http://localhost:8081}"
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
data_source_update_max_heartbeat_interval_seconds=3600

# Asset cleanup job settings (cron default is daily at 04:00).
asset_cleanup_cron="0 0 4 * * *"
asset_cleanup_max_heartbeat_interval_seconds=3600

[asset_storage]
endpoint = "http://minio:9000"
access_key = "minioadmin"
secret_key = "minioadmin"
bucket = "bike-counter-images"
region = "us-east-1"

[maps]
update_cron = "0 0 3 1 1,3,5,7,9,11 *"
update_max_heartbeat_interval_seconds = 3600
protomaps_build_url = "https://build.protomaps.com/20260829.pmtiles"
go_pmtiles_version = "1.31.2"
EOF

echo "--- Clearing any leftover containers from a previous run"
docker compose -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true

echo "--- Building and starting the stack (this builds the release binary)"
BUILD_LOG="$(mktemp)"
if ! docker compose -f "${COMPOSE_FILE}" up -d --build >"${BUILD_LOG}" 2>&1; then
  echo "ERROR: docker compose up --build failed (see log tail)" >&2
  tail -n 60 "${BUILD_LOG}" >&2 || true
  rm -f "${BUILD_LOG}"
  exit 1
fi
rm -f "${BUILD_LOG}"

# The backend builds the basemap at startup on first run (no skip possible),
# which can take minutes; allow up to 8 minutes for readiness.
echo "--- Waiting for ${APP_URL}/health/ready"
READY=0
for _ in $(seq 1 240); do
  if curl --fail --silent "${APP_URL}/health/ready" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 2
done
if [ "${READY}" -ne 1 ]; then
  echo "ERROR: app did not become ready at ${APP_URL}/health/ready" >&2
  docker compose -f "${COMPOSE_FILE}" logs backend 2>/dev/null | tail -n 40 || true
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

# The BFF API is frontend-only; verify the station list and the summary answer.
BFF_STATIONS="/api/bff/stations?min_lat=51&min_lng=7&max_lat=52&max_lng=8"
assert_status 200 "${BFF_STATIONS}"
BFF_JSON="$(curl --silent "${APP_URL}${BFF_STATIONS}")"
if ! echo "${BFF_JSON}" | grep -q '"items"'; then
  echo "ERROR: expected a station list in ${BFF_STATIONS}" >&2
  echo "${BFF_JSON}" >&2
  exit 1
fi
echo "Verified ${BFF_STATIONS} returns a station list"

assert_status 200 "/api/bff/stations/sidebar?min_lat=51&min_lng=7&max_lat=52&max_lng=8"
echo "Verified /api/bff/stations/sidebar answers"

# The frontend must serve the SPA shell (React renders "Hello World"
# client-side, so only the mount point is present in the raw HTML).
if ! curl --fail --silent "${FRONTEND_URL}/" | grep -q '<div id="root"></div>'; then
  echo "ERROR: frontend did not serve the SPA shell at ${FRONTEND_URL}" >&2
  exit 1
fi
echo "Verified frontend SPA shell at ${FRONTEND_URL}"

# The nginx reverse proxy must forward /api to the backend: reaching the BFF
# station list through the frontend URL proves browser -> nginx -> backend works.
if ! curl --fail --silent "${FRONTEND_URL}${BFF_STATIONS}" | grep -q '"items"'; then
  echo "ERROR: nginx did not proxy /api/bff/stations to the backend" >&2
  exit 1
fi
echo "Verified nginx proxies /api/bff/stations to the backend"

# The scheduler runs the data-source update at startup (it has never succeeded),
# so a data_source_update job owned by an instance must exist.
JOBS_JSON="$(curl --silent "${APP_URL}/api/v1/jobs")"
if ! echo "${JOBS_JSON}" | grep -q "data_source_update"; then
  echo "ERROR: expected at least one data_source_update job in /api/v1/jobs" >&2
  echo "${JOBS_JSON}" >&2
  exit 1
fi
if ! echo "${JOBS_JSON}" | grep -q "instance_id"; then
  echo "ERROR: expected jobs to expose instance_id" >&2
  echo "${JOBS_JSON}" >&2
  exit 1
fi
echo "Verified data_source_update job with instance_id in /api/v1/jobs"

psql_query() {
  docker compose -f "${COMPOSE_FILE}" exec -T "${DB_SERVICE}" \
    psql -U "${DB_USER}" -d "${DB_NAME}" -tAc "$1"
}

# jobs must expose the ownership/liveness columns (nullable uuid / TIMESTAMPTZ).
INSTANCE_TYPE="$(
  psql_query "SELECT data_type FROM information_schema.columns WHERE table_name='jobs' AND column_name='instance_id'"
)"
if [ "${INSTANCE_TYPE}" != "uuid" ]; then
  echo "ERROR: jobs.instance_id is missing or not uuid (got '${INSTANCE_TYPE}')" >&2
  exit 1
fi
HEARTBEAT_TYPE="$(
  psql_query "SELECT data_type FROM information_schema.columns WHERE table_name='jobs' AND column_name='heartbeat_at'"
)"
if [ "${HEARTBEAT_TYPE}" != "timestamp with time zone" ]; then
  echo "ERROR: jobs.heartbeat_at is missing or not TIMESTAMPTZ (got '${HEARTBEAT_TYPE}')" >&2
  exit 1
fi
# The job_locks ShedLock table must exist (atomic multi-instance claim).
JOB_LOCKS_TABLE="$(
  psql_query "SELECT to_regclass('job_locks')"
)"
if [ "${JOB_LOCKS_TABLE}" != "job_locks" ]; then
  echo "ERROR: the job_locks table is missing (got '${JOB_LOCKS_TABLE}')" >&2
  exit 1
fi
echo "Verified jobs.instance_id/heartbeat_at and the job_locks table"

# data_sources.imported_until must exist as TIMESTAMPTZ (nullable, DB-only).
IMPORTED_UNTIL_TYPE="$(
  psql_query "SELECT data_type FROM information_schema.columns WHERE table_name='data_sources' AND column_name='imported_until'"
)"
if [ "${IMPORTED_UNTIL_TYPE}" != "timestamp with time zone" ]; then
  echo "ERROR: data_sources.imported_until is missing or not TIMESTAMPTZ (got '${IMPORTED_UNTIL_TYPE}')" >&2
  exit 1
fi
echo "Verified data_sources.imported_until is TIMESTAMPTZ"

PASS=1
