# Bonn Open Data adapter (`bonn_opendata`)

Data provider for the **Bonn** bicycle counters, implemented as a
[`DataProvider`](../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter. It reads the officially published Bonn Open Data resources over
HTTP and serves counting stations, channels and hourly measurements through the
shared import pipeline. It was added in
[plan 56](../../../../../plans/56_bonn_adapter_plan.md:1).

Provider type (the value of `[data_sources.provider].type`):

```
bonn_opendata_http_provider
```

## Data sources (all official, CC0)

Bonn publishes the data on **opendata.bonn.de** (CKAN, mirrored on govdata.de);
the resources are `license_title = "CC0"` in the CKAN metadata. The adapter uses
three kinds of machine-readable resources (no scraping):

| Purpose | URL | Format |
|---|---|---|
| Station locations | `https://stadtplan.bonn.de/geojson?Thema=22640` | GeoJSON `FeatureCollection` of `Point`s (`properties.station_nr`, `properties.lage`, `[lon, lat]`) |
| Current measurements (Vortag) | `https://stadtplan.bonn.de/csv?OD=4285` | Semicolon CSV, header `station_id;wann;wann_datum;anzahl_raeder;uhrzeit;lage` |
| Historical backfill 2023 | `https://opendata.bonn.de/sites/default/files/Fahrradzaehlstellen2023_stuendlich.csv` | Wide yearly hourly CSV |
| Historical backfill 2024 | `https://opendata.bonn.de/sites/default/files/MessergebnisseFahrradzaehlstationenStundenauswertung2024.csv` | Wide yearly hourly CSV |
| Historical backfill 2025 | `https://opendata.bonn.de/sites/default/files/fahrradzaehldatenbonn2025.csv` | Wide yearly hourly CSV |

CKAN dataset names (for reference): `standorte-der-fahrradmessstellen-radzählungen`
(stations), `fahrradmessstellen-ergebnisse-radzählungen-vortag` (current data),
`fahrradmessstellen-ergebnisse-radzählungen-2023/2024/2025` (history).

**Data semantics of the source**

- **Hourly, one count per station per hour** — Bonn does **not** publish a
  direction split, so every value maps 1:1 to a single channel.
- **Missing data = absent rows**, never an explicit zero. Empty cells in the wide
  CSVs and missing rows in the Vortag CSV are skipped, never stored as `0`.
- The Vortag `wann` column is **UTC** (verified against the local display
  fields). The wide CSV `Time` column is **local Europe/Berlin** with DST
  duplicates and two timestamp formats (German month names in 2024, numeric in
  2023/2025) — converted DST-aware to UTC.

## Configuration

Read from the data source's provider vars in
[`adapter.rs`](adapter.rs:67) (missing/invalid required vars are startup errors):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `stations_url` | yes | — | GeoJSON station-locations URL |
| `measurements_url` | yes | — | Vortag measurements CSV URL (current/rolling) |
| `historical_urls` | no | (empty) | space-separated wide yearly hourly CSVs (2023–2025) |
| `max_measurement_batch_size` | no | `500` | page size |
| `cache_duration` | no | `300` | seconds to cache fetched bodies |

Example block in [`config.toml.example`](../../../../../config.toml.example:39).

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`fetcher.rs`](fetcher.rs:1) — `ResourceFetcher` trait (`fetch(&str) -> Result<String, String>`)
  with a `ureq`-based `HttpResourceFetcher`; tests inject a fake.
- [`parsing.rs`](parsing.rs:1) — all parsers: GeoJSON stations, Vortag CSV, wide
  yearly CSV, name normalisation/alias table, DST-aware `berlin_to_utc`,
  `build_index` (merging + dedup), `parse_host_and_port`.
- [`adapter.rs`](adapter.rs:1) — `BonnOpendataAdapter` + the `DataProvider` impl,
  config parsing, in-memory cache.
- [`tests.rs`](tests.rs:1) — unit tests (27): config, parsers, merging, paging,
  cache, health, messages, factory wiring.

## Design decisions

1. **One channel per imported station, `external_id = station_nr`.** The GeoJSON
   and the measurement CSVs share **no numeric id** (Vortag uses a 9-digit
   `station_id`, the historical CSVs have no station id at all), so rows are
   joined by the `lage` / normalized column **name**. The `station_nr` from the
   GeoJSON becomes the uniform channel external id.
2. **Aggregate stations are excluded by name marker, not hard-coded numbers.**
   The three `(errechnete Gesamtzahl)` stations (Kennedybrücke, Südbrücke,
   Nordbrücke) are dropped via `is_aggregate(name)` so the global summary is not
   double-counted. Historical aggregate columns (`Summe`, `Kennedybrücke` /
   `Nordbrücke` / `Südbrücke`) are skipped the same way.
3. **Never sum, never fabricate.** No direction columns exist to sum; missing
   values stay missing. Genuine `0` values are imported.
4. **Historical columns are mapped by name with an alias table.** `N.NN ` index
   prefixes are stripped and renamed stations are resolved (e.g. `Bröhltalweg` →
   `Bröltalbahnweg`, `Mc Cloy Weg` → `John-J.-McCloy-Ufer`, `Weg auf Damm Neil` →
   `Hochwasserdamm Beuel`, `…(Südseite) Barometer` suffix). Columns that still
   do not match an imported station are skipped.
5. **Vortag wins on overlapping hours.** The Vortag CSV (current, mutable) takes
   precedence over the historical backfill via a stable sort + keep-last dedup on
   `(channel, timestamp)`.
6. **In-memory cache, no persistent state.** All resources are re-fetched and
   re-parsed when the `cache_duration` elapses (guarded by a refresh lock, single
   lock acquisition to avoid reentrant deadlock). `attach_persistent_state` is
   not overridden.
7. **Cursor safety.** `get_measurements` filters `timestamp > from` (exclusive)
   and never synthesises a cursor on an empty window, so `imported_until` never
   jumps into the future (consistent with plan 47).
8. **Health check** is a TCP connect to the host/port of `measurements_url`
   (`stadtplan.bonn.de:443`).

## Provider messages

- `INFO` — one-line lifecycle on cache refresh (station/channel/row counts).
- `WARNING` — a genuinely unmatched non-aggregate Vortag `lage` or a normalized
  historical column that does not resolve to an imported station. The expected
  aggregate rows are skipped **silently** (they are intentional exclusions).
- `DEBUG` — malformed rows / non-point geometry (known quirks).

## Limitations

- **No 2026 history.** Verified 2026-08-27 (reference implementation + live HTTP
  tests): no 2026 bulk file exists on opendata.bonn.de, govdata or the OGD
  Cockpit, and the Vortag feed is a **rolling ~2–3-day window that ignores date
  parameters** (`datum=`, `date=`, `von=…&bis=…`). 2026 therefore accrues **one
  day at a time** through the normal hourly import. When Bonn publishes the 2026
  annual CSV (expected ~1-year lag, as with 2025), add its URL to
  `historical_urls` and reset the Bonn `imported_until` watermark to backfill the
  whole year — no adapter changes needed. Full detail in the
  [plan addendum](../../../../../plans/56_bonn_adapter_plan.md:299).
- **2015–2022 not enabled.** Their govdata resource URLs resolve to Drupal HTML
  pages; documented but not imported. Adding them later is a `historical_urls`
  change.
- **Source sparseness is preserved.** Stations such as `Straßburger Weg` (no
  published data), `Brühler Straße` (no 2024), `Rhenusallee` / `Wilhelm-Spiritus-Ufer`
  (sparse hours), and `Joseph-Beuys-Allee` / `Rheinweg` (new in 2026) import fewer
  rows than the busiest stations because Bonn simply does not publish more.
- **First import is large.** The 2023–2025 backfill is ~0.4M rows; the
  `data_source_update_max_heartbeat_interval_seconds` may need to be raised for the
  first run.
- **Name-based join is brittle by nature** — a station rename not covered by the
  alias table degrades to a `WARNING` and its rows are skipped (no invented data).
- **Health check only verifies network reachability**, not content validity.

## Testing

Unit tests in [`tests.rs`](tests.rs:1) run without network (fixtures + fake
fetcher). Validation against the live sources (manual, once) is described in the
[plan](../../../../../plans/56_bonn_adapter_plan.md:267). Gates: `make check`,
`make test`, `make test-rest`, `make coverage`.
