#!/usr/bin/env bash
# Reproduces the plan-164 workstream D query evidence (`EXPLAIN (ANALYZE,
# BUFFERS)`) against a throwaway Postgres that carries the real migrations and a
# synthetic dataset:
#
#   * 5,000 channels on 1,000 counting stations (so the unscoped
#     `find_all`-style load reads 20x more rows than a viewport needs),
#   * 3.65M `measurement_daily` rollup rows (5,000 channels x 730 local days),
#   * one timezone group, i.e. the shape `metric_windows` now issues: one
#     aggregate query for all channels of a timezone instead of one per station.
#
# It prints three plans:
#   1. the previous behaviour (`find_all`: whole `channels` table),
#   2. the new scoped query (`ChannelRepository::find_by_station_ids`),
#   3. the batched rollup aggregate (`sum_daily_by_channel` for a whole group).
#
# The container is ephemeral (unique name, no published ports) and removed on
# exit, so it cannot collide with a development or e2e stack.
#
# Usage: ./scripts/explain-readpaths.sh
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER="bc_explain_164_$$"
IMAGE="${EXPLAIN_PG_IMAGE:-postgres:18-alpine}"

cleanup() {
  docker rm -f "${CONTAINER}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

psql_run() {
  docker exec -i -e PGPASSWORD=explain "${CONTAINER}" \
    psql -v ON_ERROR_STOP=1 -U postgres -d bike_counter "$@"
}

# Prints a plan without the (very long) uuid array literals: an unaligned psql
# format keeps the plan tree one node per line, which pastes cleanly into the
# plan file.
plan() {
  psql_run -A -f - | sed -E "s/'\{[0-9a-f-]{36}(,[0-9a-f-]{36})+\}'::uuid\[\]/'{...}'::uuid[]/g"
}

echo "--- Starting ${IMAGE} in ${CONTAINER} (no published ports)"
docker run -d --name "${CONTAINER}" \
  -e POSTGRES_PASSWORD=explain \
  -e POSTGRES_DB=bike_counter \
  "${IMAGE}" >/dev/null

# A fresh data directory first runs the entrypoint's temporary server, so wait
# for a query that succeeds on the *final* server.
ready=0
for _ in $(seq 1 90); do
  if psql_run -tAc "SELECT 1" >/dev/null 2>&1; then
    sleep 2
    if psql_run -tAc "SELECT 1" >/dev/null 2>&1; then
      ready=1
      break
    fi
  fi
  sleep 1
done
if [[ "${ready}" -ne 1 ]]; then
  echo "explain-readpaths: database did not become ready" >&2
  exit 1
fi

echo "--- Applying migrations"
while read -r migration; do
  psql_run -q -f - <"${migration}" >/dev/null
done < <(ls "${PROJECT_ROOT}"/backend/migrations/*.sql | sort -V)

echo "--- Seeding synthetic data (5,000 channels, 3.65M daily rollup rows)"
psql_run -q -f - >/dev/null <<'SQL'
INSERT INTO data_sources (id, name, provider_type)
VALUES ('00000000-0000-0000-0000-0000000000aa', 'explain', 'eco_counter_v1_http_provider');

-- 1,000 stations in one timezone, ids ordered by their numeric suffix so the
-- viewport below is a contiguous id range (a plain index range scan).
INSERT INTO counting_stations (id, name, description, timezone, status, data_source_id)
SELECT ('00000000-0000-0000-0000-' || lpad(to_hex(g), 12, '0'))::uuid,
       'Station ' || lpad(g::text, 5, '0'),
       'synthetic',
       'Europe/Berlin',
       'active',
       '00000000-0000-0000-0000-0000000000aa'
FROM generate_series(1, 1000) AS g;

-- Five channels per station => 5,000 channels.
INSERT INTO channels (id, counting_station_id, name, description)
SELECT ('00000000-0000-0000-0001-' || lpad(to_hex(g), 12, '0'))::uuid,
       ('00000000-0000-0000-0000-' || lpad(to_hex(((g - 1) / 5) + 1), 12, '0'))::uuid,
       'Direction ' || (((g - 1) % 5) + 1),
       'synthetic'
FROM generate_series(1, 5000) AS g;

-- Two years of daily rollups per channel.
INSERT INTO measurement_daily (channel_id, resolution_seconds, local_date, total)
SELECT c.id,
       3600,
       d::date,
       100
FROM channels c
CROSS JOIN generate_series(DATE '2024-01-01', DATE '2025-12-31', INTERVAL '1 day') AS d;

ANALYZE;
SQL

# A viewport of 50 stations (a realistic map window: 250 of the 5,000 channels).
STATION_IDS="$(psql_run -tAc "SELECT array_agg(id)::text FROM (SELECT id FROM counting_stations ORDER BY id LIMIT 50) s")"
# One timezone group: every channel of 50 stations (metric_windows batches these).
CHANNEL_IDS="$(psql_run -tAc "SELECT array_agg(id)::text FROM (SELECT id FROM channels ORDER BY id LIMIT 250) c")"

echo
echo "=== 1) Before: find_all (whole channel table) ==="
plan <<'SQL'
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF)
SELECT id, counting_station_id, name, description, external_datasource_id
FROM channels ORDER BY name ASC;
SQL

echo
echo "=== 2) After: find_by_station_ids (scoped to 50 stations) ==="
plan <<SQL
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF)
SELECT id, counting_station_id, name, description, external_datasource_id
FROM channels
WHERE counting_station_id = ANY('${STATION_IDS}'::uuid[])
ORDER BY name ASC;
SQL

echo
echo "=== 3) After: sum_daily_by_channel (250 channels, one month) ==="
plan <<SQL
EXPLAIN (ANALYZE, BUFFERS, COSTS OFF)
SELECT channel_id, COALESCE(SUM(total), 0)::bigint AS total
FROM measurement_daily
WHERE channel_id = ANY('${CHANNEL_IDS}'::uuid[])
  AND local_date >= (TIMESTAMPTZ '2025-11-01T00:00:00Z' AT TIME ZONE 'Europe/Berlin')::date
  AND local_date <= (TIMESTAMPTZ '2025-11-30T00:00:00Z' AT TIME ZONE 'Europe/Berlin')::date
  AND (3600::bigint IS NULL OR resolution_seconds = 3600::bigint)
GROUP BY channel_id
ORDER BY channel_id;
SQL

echo
echo "explain-readpaths: OK"
