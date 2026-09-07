# Plan 30 — Quiet make output + local "last day" summary

Status: implemented

## Context

Two user requests plus the design refinements agreed during clarification:

1. **Make commands print too much.** Reduce output in the Make targets and the
   shell scripts they call (quiet cargo flags, redirect docker/npm build logs,
   tail only failures).
2. **"Last 24 h" becomes "last day".** The frontend header and station list show
   `bikes / 24 h`, backed by a rolling `now - 24h -> now` window in the BFF. It
   must instead show the **last local day**, computed in the counting station's
   own timezone (DST-aware).

## Agreed design decisions

- **Keep `TIMESTAMPTZ` storage.** Measurements stay absolute instants in UTC
  (`measurements.timestamp TIMESTAMPTZ`, [`measurement.rs`](../backend/src/core/domain/measurements/measurement.rs:20)).
  This is already DST-safe. The local-day boundary is computed at query time by
  converting the station's IANA timezone, so no measurement migration or
  re-import is needed.
- **Timezone is a per-counting-station property** (IANA string, e.g.
  `Europe/Berlin`). A provider may serve stations from different timezones, so
  the timezone travels on the station record, not the data source/provider.
- **The domain `MeasurementRepository::sum(from, to, channel_id)` stays
  generic** ("count from t1 to t2"). The *application* summary services
  ([`station_summary_service.rs`](../backend/src/core/application/station_summary_service.rs:22),
  [`global_summary_service.rs`](../backend/src/core/application/global_summary_service.rs:22))
  compute each station's local-day window and call `sum` with it.
- **"last day" = the previous complete local calendar day** (yesterday
  `00:00 -> 24:00` in the station's timezone, converted to UTC). DST is handled
  by `chrono_tz`: the window is 23 h / 25 h on transition days.
  > OPEN DECISION: confirm "yesterday (previous complete day)" vs
  > "today so far (local 00:00 -> now)". The plan implements "yesterday".

## Part 1 — Quiet make output

- [`Makefile`](../Makefile:17): add `--quiet` to the direct `cargo` invocations
  (`build`, `test`, `test-rest`, `fmt`) so compilation chatter is suppressed
  while warnings/errors still surface.
- [`scripts/fmt-test.sh`](../scripts/fmt-test.sh:17): run
  `cargo fmt --check` and `cargo clippy ...` with `--quiet`; keep the explicit
  `echo` section markers and failure output.
- [`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh:78) and
  [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh:91): redirect
  `docker compose up -d --build` to a log file, `tail -n 40` on failure only;
  silence `npm ci` and `npx playwright install chromium` unless they fail.
- [`scripts/coverage.sh`](../scripts/coverage.sh:57): keep the coverage gate
  summary lines; pass `--quiet` through to the underlying cargo run where
  supported.

## Part 2 — Local "last day" summary

### Domain model

- [`counting_station.rs`](../backend/src/core/domain/counting_stations/counting_station.rs:2):
  add `pub timezone: value_objects::Timezone` to `CountingStation` and a
  `Timezone(pub String)` value object (IANA name).
- [`provider_port.rs`](../backend/src/core/domain/data_source/provider_port.rs:87):
  add `pub timezone: String` to `CountingStationRecord`.
- [`station_summary/mod.rs`](../backend/src/core/domain/station_summary/mod.rs:13):
  rename `bikes_last_24h` -> `bikes_last_day`.
- [`global_summary/mod.rs`](../backend/src/core/domain/global_summary/mod.rs:20):
  rename `bikes_last_24h_total` -> `bikes_last_day_total`.

### Persistence

- New migration `backend/migrations/V11__add_counting_station_timezone.sql`:
  `ALTER TABLE counting_stations ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';`
- [`counting_station_repository.rs`](../backend/src/adapter/driven/postgres/counting_station_repository.rs:8):
  add `timezone` to `STATION_COLUMNS`, the INSERT/UPDATE statements and
  `map_row`.

### Provider (Münster)

- [`parsing.rs`](../backend/src/adapter/driven/muenster_github/parsing.rs:41):
  set `timezone: "Europe/Berlin".to_string()` on each `CountingStationRecord`
  built by `parse_site_index` (reuses the existing
  [`TIMEZONE`](../backend/src/adapter/driven/muenster_github/parsing.rs:18) const).

### Import / upsert

- [`data_import_service.rs`](../backend/src/core/application/data_import_service.rs:104):
  in `sync_counting_stations`, persist `record.timezone` on new stations and
  refresh it on the update path.

### Summary computation (the core of the change)

- Add a shared, DST-aware helper that resolves an IANA timezone to `Tz` and
  computes `(from, to)` for the previous local day, converting both edges to
  UTC. Half-open `[from, to)`; the existing `sum` uses `<= to`, so the upper
  edge must exclude the first instant of today.
- [`station_summary_service.rs`](../backend/src/core/application/station_summary_service.rs:43):
  change `compute` so each station is summed over its **own** local-day window
  (derived from `station.timezone`) instead of one shared `(from, to)`.
- [`global_summary_service.rs`](../backend/src/core/application/global_summary_service.rs:45):
  sum per-station local-day totals across all stations (correct when stations
  span timezones), replacing the single whole-table `sum`.

### BFF

- [`bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:67): remove
  `last_24h_window()`; the sidebar/search/global-summary handlers no longer pass
  a shared window and instead let the services compute per-station windows.
- [`bff/dto.rs`](../backend/src/adapter/driving/bff/dto.rs:31): rename
  `bikes_last_24h` -> `bikes_last_day` and `bikes_last_24h_total` ->
  `bikes_last_day_total`.

### Frontend

- [`header/types.ts`](../frontend/src/features/header/types.ts:5) and
  [`stations/types.ts`](../frontend/src/features/stations/types.ts:21): rename
  the JSON field mappings.
- [`TopBar.tsx`](../frontend/src/features/header/TopBar.tsx:33) and
  [`StationListItem.tsx`](../frontend/src/features/stations/StationListItem.tsx:37):
  change the visible text from `bikes / 24 h` to `bikes / last day`.

### Tests & docs

- Update domain unit tests, the BFF/REST tests
  ([`bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:105),
  [`fixtures.rs`](../backend/src/adapter/driving/rest/tests/fixtures.rs:1),
  [`mocks.rs`](../backend/src/adapter/driving/rest/tests/mocks.rs:1)) and the
  Postgres/adapter/import tests for the renamed fields and the new `timezone`
  field.
- Verify the Playwright specs still pass (none assert on the `24 h` text).
- Update [`agents.md`](../agents.md:17) to document the quieter scripts and the
  tail-on-failure convention, plus `README.md`/`ToDo.md` as needed.

## Gates (per [`agents.md`](../agents.md:19))

- [x] `make check`
- [x] `make test` / `make test-rest`
- [x] `make coverage`
- [x] `make test-playwright` (frontend UI changed)
- [x] docs updated
