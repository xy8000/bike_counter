# 56 - Bonn Open Data bicycle counter adapter

Status: implemented

## Problem

The application currently supports exactly one external data source (Münster,
via the `muenster_github` provider). This plan adds Bonn's **officially
published** bicycle counting data as a second data source, following the
existing [`DataProvider`](../backend/src/core/domain/data_source/provider_port.rs:151)
architecture and the adapter guide in
[`CONTRIBUTING.md`](../CONTRIBUTING.md:30). No new architecture, abstraction,
database model, scheduling mechanism or configuration structure is introduced —
the Bonn provider plugs into the existing ports, factory, import pipeline and
config layout.

Bonn's app starts without an `imported_until` cursor, so on the **first** import
the provider serves everything it has. The provider therefore serves the current
("Vortag") measurements **plus the published historical years** (2023–2025) as an
hourly backfill; the incremental `imported_until` watermark then keeps only the
rolling current data flowing afterwards.

## Source inspection (performed against the live endpoints)

Bonn publishes the data on **opendata.bonn.de** (CKAN) with a mirror on
**govdata.de**. The machine-readable resources are linked from the dataset
metadata (`/api/3/action/package_show`), and the relevant datasets are **CC0**
(`license_title = "http://www.opendefinition.org/licenses/cc-zero"` in the CKAN
metadata).

### Current station locations — `standorte-der-fahrradmessstellen-radzählungen`

- Resource: **GeoJSON** at `https://stadtplan.bonn.de/geojson?Thema=22640`.
- Format: a `FeatureCollection` of `Point` features (20 stations). Each feature:
  - `properties.station_nr` — integer station number (e.g. `11`).
  - `properties.lage` — station name (e.g. `BN - Brühler Straße`).
  - `geometry.coordinates` — `[longitude, latitude]` (WGS84, EPSG:4326).
- Three stations are aggregate counters marked `(errechnete Gesamtzahl)`
  (Kennedybrücke nr 16, Südbrücke nr 17, Nordbrücke nr 20). They are **excluded
  from the import** so the frontend global summary is not double-counted.

### Current measurements — `fahrradmessstellen-ergebnisse-radzählungen-vortag`

- Resource: **CSV** at `https://stadtplan.bonn.de/csv?OD=4285`.
- Format: **semicolon**-delimited, no BOM, header
  `station_id;wann;wann_datum;anzahl_raeder;uhrzeit;lage`.
  - `station_id` — 9-digit numeric string (e.g. `100019720`).
  - `wann` — naive ISO datetime `YYYY-MM-DDTHH:MM:SS`, **UTC** (see below).
  - `wann_datum` / `uhrzeit` — redundant local display fields (ignored).
  - `anzahl_raeder` — integer hourly count; **missing data = absent rows**.
  - `lage` — station name; the join key to the GeoJSON.
- Interval: **hourly**; **no direction split** (one count per station per hour).

### Historical measurements (hourly backfill) — govdata datasets per year

Only **2023, 2024 and 2025** are published as clean machine-readable wide CSVs
(hosted on opendata.bonn.de); these are the years the adapter enables:

| Year | Hourly CSV |
|---|---|
| 2023 | `https://opendata.bonn.de/sites/default/files/Fahrradzaehlstellen2023_stuendlich.csv` |
| 2024 | `https://opendata.bonn.de/sites/default/files/MessergebnisseFahrradzaehlstationenStundenauswertung2024.csv` |
| 2025 | `https://opendata.bonn.de/sites/default/files/fahrradzaehldatenbonn2025.csv` |

Common **wide** format (different from the Vortag CSV):
- Rows 1–2: a title line (`Zeitraum;1. Januar YYYY -> 31. Dezember YYYY;`) and a
  blank line; row 3 is the header `Time;<5.01 Name>;…;<aggregate columns>`.
- One column **per station**, identified by a per-file index prefix `5.01 … 5.15`
  followed by the station name; the last columns are **aggregates**
  (`Summe` in 2024; `Kennedybrücke;Nordbrücke;Südbrücke` in 2023/2025).
- First column `Time`: **local Europe/Berlin time** with **DST duplicates**
  (e.g. `31. März 2024 03:00` appears twice) and **two timestamp formats**:
  2024 uses `1. Jan. 2024 00:00` (German month name), 2023/2025 use
  `01.01.2025 00:00` (numeric).
- Empty cells = missing measurements; interval **hourly**.
- **No `station_id`**; historical columns must be mapped to stations **by name**.
  The display names are **not always identical** to today's names: e.g.
  `Bröhltalweg` vs current `Bröltalbahnweg`, `Mc Cloy Weg` vs
  `John-J.-McCloy-Ufer`, `Weg auf Damm Neil` vs `Hochwasserdamm Beuel`,
  `…(Südseite) Barometer` suffix; `5.12 Straßburger Weg` exists only in 2024.

Years **2015–2022** exist on govdata but their resource URLs resolve to Drupal
**HTML pages** (old portal) rather than direct files — they are **documented but
not enabled** (see [Historical scope](#historical-scope)).

### Timestamp semantics (verified)

- `wann` (Vortag CSV) is **UTC**: `wann=2026-08-23T22:00:00` equals
  `wann_datum=24.08.2026` + `uhrzeit=00:00 Uhr` (local CEST = UTC+2).
- Historical `Time` values are **local Europe/Berlin** and must be converted to
  UTC DST-aware (as Münster does), with the DST duplicate hour handled by
  `single()` → `earliest()`.

## Data semantics

- **Do not sum.** The Vortag CSV has a single `anzahl_raeder` per station per
  hour; historical columns are a single value per station per hour. There is no
  direction split, so values map 1:1 to a channel.
- **Exclude computed-total data.** The three `(errechnete Gesamtzahl)` stations
  (by the name marker, not hard-coded numbers) and the historical aggregate
  columns (`Summe`, `Kennedybrücke`/`Nordbrücke`/`Südbrücke`) are excluded, so
  the global summary is not double-counted. Historical aggregate columns are
  skipped because their normalized names do not match an imported station.
- **Do not invent zeros.** Missing hours (absent cells/rows) are skipped, never
  stored as `0`. Genuine `0` values are imported.
- **Renamed historical stations** are mapped to today's stations through a small
  **alias table** (documented in code); columns that still do not match an
  imported station are skipped with a `WARNING`.

## Historical scope

- **Enabled:** the machine-readable hourly backfill for **2023, 2024, 2025**
  (wide CSVs above), driven by the configured `historical_urls` list.
- **Documented, not enabled:** 2015–2022 (Drupal-era HTML resource pages,
  heterogeneous formats). Adding them later only requires listing their direct
  file URLs in `historical_urls`; no adapter changes are expected beyond the
  timestamp/numbering variants they may introduce.
- **Current year (2026): no bulk file exists yet** — see
  [Addendum](#addendum-current-year-2026-availability-verified-2026-08-27). The
  only 2026 data source is the rolling Vortag CSV, which the adapter already
  accumulates day by day through the normal incremental import.

## Fix design

### 1. New adapter module `backend/src/adapter/driven/bonn_opendata/`

Follows the Münster module split (config + `DataProvider` impl in `adapter.rs`,
HTTP abstraction in `fetcher.rs`, parsers in `parsing.rs`, tests in
`tests.rs`). Provider type: **`bonn_opendata_http_provider`**.

**Config vars** (parsed in `new(&DataSourceConfiguration)`, fail fast on
missing/invalid):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `stations_url` | yes | — | GeoJSON station-locations URL |
| `measurements_url` | yes | — | Vortag measurements CSV URL (current) |
| `historical_urls` | no | (empty) | space-separated wide-format yearly hourly CSVs (2023–2025) |
| `max_measurement_batch_size` | no | `500` | page size |
| `cache_duration` | no | `300` | seconds to cache the fetched bodies |

**`fetcher.rs`** — a small `ResourceFetcher` trait
(`fn fetch(&self, url: &str) -> Result<String, String>`) with a `ureq`-based
`HttpResourceFetcher`; tests inject a fake (mirrors
[`fetcher.rs`](../backend/src/adapter/driven/muenster_github/fetcher.rs:1)).

**`parsing.rs`**:
- `parse_stations_geojson(&str, messages)` — parses the `FeatureCollection`;
  `station_nr` → `external_id` (number or string), `lage` → `name`, `[lon, lat]`
  → coordinates (`Point` only, else `None`), timezone `Europe/Berlin`,
  `image_sha256: None`.
- `parse_measurements_csv(&str, messages)` — the Vortag CSV (semicolon):
  `station_id`, `wann` (`%Y-%m-%dT%H:%M:%S`, interpreted as **UTC**),
  `anzahl_raeder`, `lage`. Malformed rows are skipped (DEBUG); a header missing a
  required column is `InvalidData`.
- `parse_yearly_hourly_csv(&str, station_by_name, messages)` — the **wide**
  historical CSV: skips the two preamble rows, reads the header, and for each
  data row emits one row per column whose **normalized** header maps to an
  imported station. Timestamps are parsed in both German formats and converted
  local→UTC DST-aware. Aggregate/unmatched columns are skipped.
- `normalize_column_name(&str) -> Option<String>` — strips the `N.NN ` index
  prefix and applies the **alias table** (e.g. `Bröhltalweg` →
  `Bröltalbahnweg`).
- `berlin_to_utc(&NaiveDateTime) -> Option<DateTime<Utc>>` — DST-aware (reuses
  the Münster approach).
- `build_index(stations, vortag_rows, yearly_rows)` — drops the
  `(errechnete Gesamtzahl)` stations (by the `is_aggregate(name)` marker),
  creates **one channel per imported station** (`external_id = station_nr`,
  `name = lage`), and merges all measurement sources keyed by `station_nr`:
  Vortag rows via `lage → station_nr`, historical rows via
  `normalize_column_name → station_nr`. Rows are merged with the **Vortag CSV
  taking precedence on overlapping hours**, deduplicated on
  `(channel, timestamp)`, and sorted ascending.

**`adapter.rs`** — `BonnOpendataAdapter` implementing [`DataProvider`](../backend/src/core/domain/data_source/provider_port.rs:151):

- In-memory cache `Mutex<Option<CachedData>>` (`fetched_at: Instant` + the
  parsed index), refreshed under a `refresh_lock` after `cache_duration`; emits
  an `INFO` lifecycle message on refresh. No persistent state (re-fetching is
  cheap); `attach_persistent_state` is not overridden.
- `get_all_counting_stations` / `get_all_channels` return the cached records.
- `get_measurements(query)`:
  - resolves `query.channel.external_datasource_id` (= `station_nr`);
  - filters the channel's merged rows to `timestamp > query.from` (exclusive)
    and `timestamp <= query.to` when given;
  - truncates to `max_batch_size`; `batch_size_limit_reached` when more remain;
  - `last_measurement_datetime` = last returned timestamp (or `None` when empty);
  - `timeframe_limit_reached = false`; no synthetic cursor on an empty window, so
    `imported_until` never jumps into the future (consistent with plan 47).
- `check_health` — TCP connect to the host/port of `measurements_url`.
- `attach_provider_messages` + `emit` — mirrors Münster.

**Provider messages**:
- `INFO` — one-line lifecycle on refresh (stations/channels/rows counts).
- `WARNING` — a Vortag `lage` or a normalized historical column that does not
  resolve to an imported station (channel/data skipped).
- `DEBUG` — malformed rows / non-point geometry skipped (known quirks).

### 2. Register the provider

- [`backend/src/adapter/driven/mod.rs`](../backend/src/adapter/driven/mod.rs:1):
  add `pub mod bonn_opendata;`.
- [`backend/src/adapter/driven/data_provider_factory.rs`](../backend/src/adapter/driven/data_provider_factory.rs:14):
  add a `BonnOpendataAdapter::provider_type()` match arm + a
  `builds_bonn_provider_type` test.

### 3. Configuration

- [`config.toml.example`](../config.toml.example:25): add a second active
  `[[data_sources]]` entry for Bonn with `stations_url`, `measurements_url`, and
  the three 2023–2025 `historical_urls`. The e2e/compose-test scripts write their
  own `config.toml`, so enabling Bonn in the example does not affect CI.
- Note: the **first** Bonn import backfills ~3 years of hourly data (≈ 0.4 M
  rows). `data_source_update_max_lifetime_seconds` in the example may need to be
  raised for that first run.

### 4. Documentation

- [`README.md`](../README.md:1): update the intro (Münster + Bonn), the
  data-source section (Bonn provider type, vars, CC0 license, stations from
  GeoJSON, hourly Vortag CSV with UTC `wann`, the 2023–2025 hourly backfill via
  wide CSVs with DST-aware local timestamps and a name alias table, excluded
  aggregate stations/columns, missing data = absent rows), and the
  incremental-update paragraph (Vortag CSV is a ~2-day rolling window; 2015–2022
  documented but not enabled).

## File changes

- `backend/src/adapter/driven/bonn_opendata/mod.rs` (new)
- `backend/src/adapter/driven/bonn_opendata/adapter.rs` (new)
- `backend/src/adapter/driven/bonn_opendata/fetcher.rs` (new)
- `backend/src/adapter/driven/bonn_opendata/parsing.rs` (new)
- `backend/src/adapter/driven/bonn_opendata/tests.rs` (new)
- `backend/src/adapter/driven/mod.rs` — module registration
- `backend/src/adapter/driven/data_provider_factory.rs` — factory arm + test
- `config.toml.example` — Bonn data source
- `README.md` — docs
- `plans/README.md` — register this plan

No migrations, no core changes, no REST/BFF changes, no frontend changes.

## Testing

Unit tests in `bonn_opendata/tests.rs` (in-memory mocks + fixture files, per
[`CONTRIBUTING.md`](../CONTRIBUTING.md:150)):

1. Config: missing/invalid `stations_url`/`measurements_url` → `ConfigError`;
   defaults for `max_measurement_batch_size` and `cache_duration`; `historical_urls`
   parsed as a list; custom values read.
2. `parse_stations_geojson`: fixture FeatureCollection → stations with
   `station_nr`, `lage`, lon/lat; non-Point geometry → `None` coordinates;
   `(errechnete Gesamtzahl)` stations dropped.
3. `parse_measurements_csv`: fixture Vortag CSV → UTC timestamps (assert
   `2026-08-23T22:00:00` → `2026-08-23T22:00:00+00:00`), values, lage; malformed
   row skipped; missing column → `InvalidData`.
4. `parse_yearly_hourly_csv`: fixture wide CSV (both `1. Jan. 2024 00:00` and
   `01.01.2025 00:00` timestamps) → DST-aware UTC rows per mapped station; empty
   cells skipped; aggregate columns (`Summe`, bridge totals) skipped; renamed
   stations mapped via the alias table; unmatched columns skipped.
5. Join/mapping: fixture sources → one channel per imported station
   (`external_id = station_nr`); Vortag rows and historical rows both keyed to
   the same channel; overlapping hours prefer the Vortag value; dedup on
   `(channel, timestamp)`.
6. `get_measurements`: filters by channel external id; `from` exclusive;
   ascending; `last_measurement_datetime` set; `batch_size_limit_reached` when a
   small batch truncates; empty window → no cursor.
7. Cache: fresh data reused (fetcher hit once across two calls); stale refetch.
8. `check_health`: `Down` for an unreachable host/port.
9. Factory: `builds_bonn_provider_type`; unknown type still rejected.

Validation against the live sources (manual, once):

1. `get_all_counting_stations` returns 17 stations (the three `(errechnete
   Gesamtzahl)` stations absent) with GeoJSON names/coordinates.
2. `get_all_channels` returns 17 channels (`external_id = station_nr`).
3. A few `anzahl_raeder` values compared with the Vortag CSV and the 2024 hourly
   CSV; historical timestamps verified DST-correct.
4. First-import paging completes the 2023–2025 backfill without advancing
   `imported_until` beyond the latest Vortag sample.

Gates: `make check`, `make test`, `make test-rest`, `make coverage`.

## Acceptance criteria

- A configured Bonn data source imports 17 stations (coordinates from the
  official GeoJSON), 17 channels and the hourly measurements — the 2023–2025
  backfill plus the rolling Vortag CSV.
- The three `(errechnete Gesamtzahl)` stations, the historical aggregate
  columns, and unmatched/renamed-unknown columns are excluded; values are never
  summed or fabricated; the Vortag value wins on overlapping hours.
- Measurements are deduplicated on `(channel_id, timestamp)`; a re-run imports
  nothing new.
- `imported_until` never advances past the latest published sample; an empty
  window leaves it untouched.
- A renamed or newly missing station degrades to a `WARNING` message without
  failing the import.
- `make check`, `make test`, `make test-rest`, `make coverage` are green.

## Addendum: current-year (2026) availability — verified 2026-08-27

Follow-up investigation (reference implementation + live HTTP tests) into whether
the complete 2026 history (2026-01-01 → current) can be retrieved, beyond the
rolling "previous day" feed. **Conclusion: it cannot — no official source serves
2026 history yet.**

Evidence:

1. **`counts_url` of the independent Lage.Bonn project is the same Vortag URL the
   adapter already uses.** [`models.py`](https://codeberg.org/machdenstaat/lage/blob/main/src/lage/models.py#L416)
   defines `counts_url = "https://stadtplan.bonn.de/csv?OD=4285"` and
   `locations_url = "https://stadtplan.bonn.de/geojson?Thema=22640"` — identical
   to `measurements_url` / `stations_url`.
2. **The feed is a rolling ~2–3-day window and ignores date parameters.** Live
   tests of the exact `counts_url` returned 773 rows spanning
   `2026-08-24T22:00` → `2026-08-26T21:00`; appending `&datum=2026-02-15`,
   `&date=2026-02-15`, or `&von=…&bis=…` produced byte-identical responses.
   February 2026 and January 2026 are therefore not retrievable.
3. **Lage.Bonn's own docs confirm this**: its daily appender states the rolling
   feed "returns exactly one day's data (with a ~2 day lag) and ignores any
   date-range parameters … It cannot backfill days it never saw."
4. **No 2026 bulk file exists** on opendata.bonn.de CKAN (`package_list` stops at
   2025 + `…-vortag`), on the govdata mirror, or in the OGD Cockpit API index.

Strategy: **D — no historical access for 2026.** The adapter's existing hourly
import of the Vortag feed already performs the day-by-day accumulation (the only
mechanism that works), idempotently via the `(channel_id, timestamp)` natural
key. When Bonn publishes the 2026 annual wide CSV (expected ~1 year lag, as with
2025), adding its URL to `historical_urls` + resetting the Bonn `imported_until`
watermark backfills the whole year with no adapter changes.
