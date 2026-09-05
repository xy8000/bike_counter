#!/usr/bin/env bash
# Playwright end-to-end browser tests against the real Docker Compose stack, using
# a committed SQL fixture instead of a live provider import.
#
# Boots PostgreSQL + backend + frontend with all seven data sources configured
# (Münster, Bonn, Hamburg, Eco-Counter, Hessen Mobil, Düsseldorf, Köln — mirroring
# config.toml) and SEEDS the database from frontend/e2e/e2e-seed.sql
# (docker-entrypoint-initdb.d via frontend/e2e/docker-compose.e2e.yml). The seed
# contains the schema, the refinery history, every counting station/channel and
# synthesized recent measurements. The scheduled background jobs are disabled via
# `scheduled_jobs_enabled = false` in the temporary config below (and the seed
# carries pre-finished jobs as well), so the backend never starts a data-import,
# asset-cleanup or tiles job — the run is fully offline w.r.t. the open-data
# providers. The backend healthcheck is overridden to /health/live so readiness
# never pings the providers either, and the committed tiles/map.pmtiles archive
# means no Protomaps download is needed.
#
# NOTE: the e2e is fully isolated from development data. It uses the dedicated
# `postgres_data_e2e`/`minio_data_e2e` volumes (see frontend/e2e/docker-compose.e2e.yml);
# the `down -v` in this script drops those e2e volumes only, so the development
# `postgres_data`/`minio_data` volumes are never touched and survive every run.
# The e2e volume is re-seeded from the fixture on each run.
#
# Requirements: Docker, Docker Compose v2 (`docker compose`), Node.js + npm
# (frontend dependencies).
#
# A temporary config.toml is created at the repo root (the backend service
# mounts ./config.toml). Any pre-existing config.toml is backed up and restored.
#
# Override the frontend URL with FRONTEND_URL (default http://localhost:8081).
set -euo pipefail

SCRIPT_DIR="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"
PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"

COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"
COMPOSE_OVERRIDE="${PROJECT_ROOT}/frontend/e2e/docker-compose.e2e.yml"
CONFIG_FILE="${PROJECT_ROOT}/config.toml"
CONFIG_BACKUP="$(mktemp)"

APP_URL="${APP_URL:-http://localhost:8080}"
FRONTEND_URL="${FRONTEND_URL:-http://localhost:8081}"

# A bounding box per city covering every seeded counting station. The BFF returns
# stations name-ordered and the seed guarantees the alphabetically-first station
# per city has measurements, so the specs' "first marker" always renders charts.
declare -A CITY_BBOX=(
  [Münster]="min_lat=51.8&min_lng=7.4&max_lat=52.1&max_lng=7.9"
  [Bonn]="min_lat=50.6&min_lng=7.0&max_lat=50.8&max_lng=7.3"
  [Hamburg]="min_lat=53.4&min_lng=9.8&max_lat=53.7&max_lng=10.2"
)

HAD_CONFIG=0
PASS=0
# Temporary build log; removed in the cleanup trap so the rm calls stay grouped.
BUILD_LOG=""

cleanup() {
  echo "--- Tearing down the Docker Compose stack"
  docker compose -f "${COMPOSE_FILE}" -f "${COMPOSE_OVERRIDE}" down -v --remove-orphans >/dev/null 2>&1 || true
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

# Test config: the database via the compose service name `db` + all seven data
# sources (the same set as config.toml). The providers are only there so the
# startup sync keeps the seeded data_sources (a configured source that is missing
# from the seed would otherwise be created, and a seeded source that is missing
# from the config would be cascade-deleted); the scheduled jobs are disabled via
# `scheduled_jobs_enabled = false`, so the backend never reaches a provider.
cat > "${CONFIG_FILE}" <<EOF
database_url="postgres://db:5432"
database_user="postgres"
database_password="postgres"
database_name="bike_counter"

# Scheduled background jobs (data-source update, asset cleanup, tiles update)
# are disabled for the offline e2e run.
scheduled_jobs_enabled = false

data_source_update_cron="0 0 * * * *"
data_source_update_max_heartbeat_interval_seconds=3600

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

[[data_sources]]
name = "Münster"
[data_sources.provider]
type = "münster_opendata_github_provider"
[data_sources.provider.vars]
url = "https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip"

[[data_sources]]
name = "Bonn"
[data_sources.provider]
type = "bonn_opendata_http_provider"
[data_sources.provider.vars]
stations_url = "https://stadtplan.bonn.de/geojson?Thema=22640"
measurements_url = "https://stadtplan.bonn.de/csv?OD=4285"
historical_urls = "https://opendata.bonn.de/sites/default/files/Fahrradzaehlstellen2023_stuendlich.csv https://opendata.bonn.de/sites/default/files/MessergebnisseFahrradzaehlstationenStundenauswertung2024.csv https://opendata.bonn.de/sites/default/files/fahrradzaehldatenbonn2025.csv"

[[data_sources]]
name = "Hamburg"
[data_sources.provider]
type = "hamburg_sta_http_provider"
[data_sources.provider.vars]
max_measurement_batch_size = "500"

[[data_sources]]
name = "Eco-Counter"
[data_sources.provider]
type = "eco_counter_v1_http_provider"

[[data_sources]]
name = "Hessen Mobil"
[data_sources.provider]
type = "eco_counter_web_http_provider"
[data_sources.provider.vars]
scrape_url = "https://hessen-mobil.eco-counter.com"
cache_duration = "3000"

[[data_sources]]
name = "Landeshauptstadt Düsseldorf | Dauerzählstellen Radverkehr"
[data_sources.provider]
type = "eco_counter_web_http_provider"
[data_sources.provider.vars]
scrape_url = "https://duesseldorf.eco-counter.com/"
cache_duration = "3000"

[[data_sources]]
name = "Stadt Köln"
[data_sources.provider]
type = "eco_counter_web_http_provider"
[data_sources.provider.vars]
scrape_url = "https://stadtkoeln.eco-counter.com/"
cache_duration = "3000"
EOF

echo "--- Clearing any leftover e2e containers and volumes from a previous run"
docker compose -f "${COMPOSE_FILE}" -f "${COMPOSE_OVERRIDE}" down -v --remove-orphans >/dev/null 2>&1 || true

echo "--- Building and starting the stack (this builds the release binary)"
BUILD_LOG="$(mktemp)"
if ! docker compose -f "${COMPOSE_FILE}" -f "${COMPOSE_OVERRIDE}" up -d --build >"${BUILD_LOG}" 2>&1; then
  echo "ERROR: docker compose up --build failed (see log tail)" >&2
  tail -n 60 "${BUILD_LOG}" >&2 || true
  exit 1
fi
echo "--- Stack built."

# The db container seeds the fixture on its fresh e2e volume
# (docker-entrypoint-initdb.d) and the backend healthcheck (overridden in
# frontend/e2e/docker-compose.e2e.yml) is /health/live, so this wait never
# touches the data providers. Allow up to 5 minutes.
echo "--- Waiting for ${APP_URL}/health/live"
READY=0
for _ in $(seq 1 60); do
  if curl --fail --silent "${APP_URL}/health/live" >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 5
done
if [ "${READY}" -ne 1 ]; then
  echo "ERROR: app did not become ready at ${APP_URL}/health/live" >&2
  docker compose -f "${COMPOSE_FILE}" logs backend 2>/dev/null | tail -n 40 || true
  exit 1
fi
echo "--- Stack started (app ready)."

# Mark one seeded Münster station inactive so the e2e suite can assert the
# inactive flag (status = inactive). The fixture itself carries the V21 schema
# (including the status column), but the seeded rows are all active — marking
# Gartenstraße here keeps the inactive-flag spec deterministic.
echo "--- Marking one seeded Münster station inactive (inactive-flag e2e test)"
docker compose -f "${COMPOSE_FILE}" -f "${COMPOSE_OVERRIDE}" exec -T db psql -U postgres -d bike_counter \
  -c "UPDATE counting_stations SET status = 'inactive' WHERE name = 'Gartenstraße';" >/dev/null 2>&1 \
  || echo "warning: could not mark Gartenstraße inactive"

echo "--- Waiting for the seeded counting stations per city"
for city in "${!CITY_BBOX[@]}"; do
  IMPORTED=0
  for _ in $(seq 1 120); do
    if curl --fail --silent "${FRONTEND_URL}/api/bff/stations?${CITY_BBOX[$city]}" | grep -q '"items":\[\]'; then
      sleep 2
    else
      IMPORTED=1
      break
    fi
  done
  if [ "${IMPORTED}" -ne 1 ]; then
    echo "ERROR: no seeded counting stations for ${city} (${FRONTEND_URL}/api/bff/stations?${CITY_BBOX[$city]})" >&2
    docker compose -f "${COMPOSE_FILE}" logs backend 2>/dev/null | tail -n 40 || true
    exit 1
  fi
  echo "${city}: counting stations present."
done

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
