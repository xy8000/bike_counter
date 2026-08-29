#!/usr/bin/env bash
# Playwright end-to-end browser tests against the real Docker Compose stack.
#
# Boots PostgreSQL + backend + frontend with a real Münster data source (the
# data-source update job runs at startup because it has never succeeded),
# waits for readiness and for the counting-station import, then runs the
# Playwright specs in frontend/e2e/ (map marker popups, search -> find on map,
# sidebar visible stations). Tears the stack down afterwards.
#
# Requirements: Docker, Docker Compose v2 (`docker compose`), Node.js + npm
# (frontend dependencies), and network access to GitHub (the Münster archive)
# and to Protomaps (the self-hosted PMTiles basemap extract; see
# tiles/README.md). The basemap is optional for the tests: the markers and
# popups render independently of the tile layer.
#
# A temporary config.toml is created at the repo root (the backend service
# mounts ./config.toml). Any pre-existing config.toml is backed up and restored.
#
# Override the frontend URL with FRONTEND_URL (default http://localhost:8081).
set -euo pipefail

SCRIPT_DIR="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"
PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"

COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"
CONFIG_FILE="${PROJECT_ROOT}/config.toml"
CONFIG_BACKUP="$(mktemp)"

APP_URL="${APP_URL:-http://localhost:8080}"
FRONTEND_URL="${FRONTEND_URL:-http://localhost:8081}"

# A bounding box covering every Münster counting station (lat ~51.9-52.0,
# lng ~7.6-7.7). The import populates the map/sidebar/search from this data.
BFF_STATIONS="/api/bff/stations?min_lat=51.8&min_lng=7.4&max_lat=52.1&max_lng=7.9"

HAD_CONFIG=0
PASS=0
# Temporary build log; removed in the cleanup trap so the rm calls stay grouped.
BUILD_LOG=""

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
  # Grouped removal of the remaining temporary files (reviewed manually).
  rm -f "${CONFIG_BACKUP}"
  [ -n "${BUILD_LOG}" ] && rm -f "${BUILD_LOG}"
  if [ "${PASS}" -eq 1 ]; then
    echo "e2e-playwright: OK"
  else
    echo "e2e-playwright: FAILED"
  fi
}
trap cleanup EXIT

# Back up an existing config.toml so it can be restored afterwards.
if [ -f "${CONFIG_FILE}" ]; then
  cp "${CONFIG_FILE}" "${CONFIG_BACKUP}"
  HAD_CONFIG=1
fi

# Test config: the database via the compose service name `db` + the Münster
# data source. Its startup import feeds the map markers, the sidebar and the
# search dialog with real stations.
cat > "${CONFIG_FILE}" <<EOF
database_url="postgres://db:5432"
database_user="postgres"
database_password="postgres"
database_name="bike_counter"

# Data-source update job settings (cron default is hourly).
data_source_update_cron="0 0 * * * *"
data_source_update_max_lifetime_seconds=3600

# Asset cleanup job settings (cron default is daily at 04:00).
asset_cleanup_cron="0 0 4 * * *"
asset_cleanup_max_lifetime_seconds=3600

[asset_storage]
endpoint = "http://minio:9000"
access_key = "minioadmin"
secret_key = "minioadmin"
bucket = "bike-counter-images"
region = "us-east-1"

[[data_sources]]
name = "Münster"

[data_sources.provider]
type = "münster_opendata_github_provider"

[data_sources.provider.vars]
url = "https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"
max_measurement_batch_size = "500"
max_measurement_timeframe_hours = "168"
EOF

echo "--- Clearing any leftover containers from a previous run"
docker compose -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true

echo "--- Building and starting the stack (this builds the release binary)"
BUILD_LOG="$(mktemp)"
if ! docker compose -f "${COMPOSE_FILE}" up -d --build >"${BUILD_LOG}" 2>&1; then
  echo "ERROR: docker compose up --build failed (see log tail)" >&2
  tail -n 60 "${BUILD_LOG}" >&2 || true
  exit 1
fi
echo "--- Stack built."

echo "--- Waiting for ${APP_URL}/health/ready"
READY=0
for _ in $(seq 1 120); do
  if curl --fail --silent "${APP_URL}/health/ready" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 2
done
if [ "${READY}" -ne 1 ]; then
  echo "ERROR: app did not become ready at ${APP_URL}/health/ready (is GitHub reachable?)" >&2
  docker compose -f "${COMPOSE_FILE}" logs backend 2>/dev/null | tail -n 40 || true
  exit 1
fi
echo "--- Stack started (app ready)."

echo "--- Waiting for the Münster counting-station import"
IMPORTED=0
for _ in $(seq 1 300); do
  if curl --fail --silent "${FRONTEND_URL}${BFF_STATIONS}" | grep -q 'Bohlweg'; then
    IMPORTED=1
    break
  fi
  sleep 2
done
if [ "${IMPORTED}" -ne 1 ]; then
  echo "ERROR: no counting stations were imported (checked ${FRONTEND_URL}${BFF_STATIONS})" >&2
  docker compose -f "${COMPOSE_FILE}" logs backend 2>/dev/null | tail -n 40 || true
  exit 1
fi
echo "Counting stations imported."

echo "--- Ensuring the frontend dependencies and the Playwright Chromium browser are installed"
if [ ! -x "${PROJECT_ROOT}/frontend/node_modules/.bin/playwright" ]; then
  echo "  npm ci (installing frontend dependencies)"
  if ! npm ci --prefix "${PROJECT_ROOT}/frontend" >/dev/null 2>&1; then
    echo "ERROR: npm ci failed (frontend dependencies)" >&2
    exit 1
  fi
fi
echo "  installing the Playwright Chromium browser"
if ! npm exec --prefix "${PROJECT_ROOT}/frontend" -- playwright install chromium >/dev/null 2>&1; then
  echo "ERROR: Playwright Chromium install failed" >&2
  exit 1
fi

echo "--- Running Playwright tests against ${FRONTEND_URL}"
FRONTEND_URL="${FRONTEND_URL}" npm exec --prefix "${PROJECT_ROOT}/frontend" -- playwright test --config "${PROJECT_ROOT}/frontend/playwright.config.ts"
echo "--- Tests finished."

PASS=1
