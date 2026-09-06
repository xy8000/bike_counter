# 109 - Leipzig WFS bicycle counter adapter

Status: implemented

## Problem

Add the City of Leipzig as a new data source via the official **geodienste.leipzig.de**
WFS (GeoServer). Leipzig publishes bicycle counts as three WFS layers with
`outputFormat=application/json` (a GeoJSON `FeatureCollection`):

- **Stations** (static locations): `OpenData:radverkehr_dauerzaehlstelle_standort_statisch`
- **Per hour**: `OpenData:radverkehr_dauerzaehlstelle_anzahl_stunde_zeitreihe`
- **Per day**: `OpenData:radverkehr_dauerzaehlstelle_anzahl_tag_zeitreihe`

The instruction "bitte beide fetchen" means the adapter must fetch **both** the
hourly and the daily time-series layers and import them at their respective
resolutions (3600 s and 86400 s) into the same per-station channel.

The implementation follows the existing [`DataProvider`](../backend/src/core/domain/data_source/provider_port.rs:151)
architecture and the adapter guide in [`CONTRIBUTING.md`](../CONTRIBUTING.md:30),
mirroring the [`bonn_opendata`](../backend/src/adapter/driven/bonn_opendata/adapter.rs:1)
module split. No new architecture, abstraction, database model, scheduling
mechanism or config structure is introduced.

## Source inspection (verified from provided sample data, 2026-09-06)

All three layers share the same GeoJSON shape produced by GeoServer WFS 2.0. A
`Feature` carries `type`, `id` (`<layer>.<objectid>`), `geometry` (`Point`),
`geometry_name: "geom"`, an optional `bbox`, and `properties`.

### Station layer (`radverkehr_dauerzaehlstelle_standort_statisch`)

```json
{"type":"Feature","id":"radverkehr_dauerzaehlstelle_standort_statisch.10371",
 "geometry":{"type":"Point","coordinates":[316626.384556,5690467.490877]},
 "geometry_name":"geom",
 "properties":{"objectid":10371, "...": "..."}}
```

- `properties.objectid` — the station row id (integer).
- `geometry.coordinates` — `[easting, northing]` in a **projected CRS**
  (ETRS89 / UTM zone 33N, EPSG:25833). Verified against the known Leipzig WGS84
  locations (see [Coordinate system](#coordinate-system)).
- The station layer carries `stationid` / `stationname` too (the time-series
  features below reference them); the parser uses `stationid` (external id) and
  `stationname` (display name), falling back to `objectid` when `stationid` is
  absent.

### Hourly layer (`radverkehr_dauerzaehlstelle_anzahl_stunde_zeitreihe`)

```json
{"type":"Feature","id":"radverkehr_dauerzaehlstelle_anzahl_stunde_zeitreihe.1337286",
 "geometry":{"type":"Point","coordinates":[316626.3846,5690467.4909]},
 "geometry_name":"geom",
 "properties":{
   "objectid":1337286,
   "stationname":"Manetstraße",
   "stationid":"de.sn.stlp.statisch.rad.100040870",
   "phenomenontime":"2026-08-06T00:00:00+02:00",
   "count":17,
   "fme_tstamp":"2026-09-06T04:10:21.698+02:00"}}
```

- `stationid` — stable external id (the numeric suffix matches the Eco-Visio
  counter id, e.g. `100040870` = Manetstraße). It is the channel key.
- `stationname` — display name.
- `phenomenontime` — **RFC 3339** timestamp with a numeric UTC offset
  (`+02:00` in summer, `+01:00` in winter). It is the **start** of the counted
  hour; the count covers that hour. Parse RFC 3339 and convert to UTC.
- `count` — the hourly bicycle count.
- `fme_tstamp` — FME ingestion timestamp; **ignored**. (It shows the layer lags
  roughly a month behind wall-clock time; the `imported_until` watermark makes
  this harmless.)

### Daily layer (`radverkehr_dauerzaehlstelle_anzahl_tag_zeitreihe`)

```json
{"type":"Feature","id":"radverkehr_dauerzaehlstelle_anzahl_tag_zeitreihe.788750",
 "geometry":{"type":"Point","coordinates":[316626.3846,5690467.4909]},
 "geometry_name":"geom",
 "properties":{
   "objectid":788750,
   "stationname":"Manetstraße",
   "stationid":"de.sn.stlp.statisch.rad.100040870",
   "phenomenontime":"2026-03-23",
   "count":4568,
   "fme_tstamp":"2026-09-06T04:11:49.048+02:00"}}
```

- Same `stationid` / `stationname` / `count` semantics as the hourly layer.
- `phenomenontime` is a **date only** (`YYYY-MM-DD`), a **calendar day** in
  Europe/Berlin local time (no offset, no time-of-day). The daily count covers
  the whole local day.

### Coordinate system

`geometry.coordinates` is `[easting, northing]` in **ETRS89 / UTM zone 33N**
(EPSG:25833), not WGS84 lon/lat. Evidence: Manetstraße publishes
`[316626.38, 5690467.49]`; the Eco-Visio catalog records the same station at
WGS84 `51.33563, 12.3676`, which is exactly what a UTM-33N inverse projection of
that easting/northing yields. All Leipzig stations lie within zone 33N (lon
≈ 12.3–12.5 °E), so a single hard-coded zone is sufficient.

The adapter therefore **converts projected coordinates to WGS84 in code** with a
small, dependency-free Transverse Mercator inverse (Snyder series, WGS84
ellipsoid, central meridian 15°E, false easting 500 000, scale 0.9996). This is
deterministic, unit-testable against known Leipzig stations, and does not rely on
the server honoring `srsName` reprojection for GeoJSON output.

## Data semantics

- **One channel per station, `external_id = stationid`.** Both the hourly and the
  daily rows for a station map to the same channel, so one station has one
  channel carrying two resolutions.
- **Mixed resolutions coexist.** Hourly rows use `resolution_seconds = 3600`,
  `interval_end = None` (fixed-second resolution). Daily rows use
  `resolution_seconds = 86400` with a DST-aware `interval_end`. The database
  natural key is `(channel_id, timestamp, resolution_seconds)` (migration
  [V14](../backend/migrations/V14__add_measurements_resolution.sql:57)) and the
  overlap guard is per `(channel_id, resolution_seconds)`, so a daily row and the
  hourly row at the same local midnight instant both persist without conflict.
- **No direction split, no summing.** `count` maps 1:1 to a measurement.
- **Do not invent zeros.** A feature without a usable `count`/`phenomenontime`
  is skipped (DEBUG); genuine `0` values are imported.
- **Daily rows are calendar-anchored.** `phenomenontime` date `D` becomes
  `timestamp` = `D 00:00 Europe/Berlin` → UTC and `interval_end` =
  `(D+1) 00:00 Europe/Berlin` → UTC, both DST-aware via `chrono_tz::Europe::Berlin`
  (already a dependency).
- **No aggregate filtering expected.** The `radverkehr_dauerzaehlstelle_*`
  layers contain the official physical counters only; the Eco-Visio computed
  `Gesamtquerschnitt` / `(berechnet)` stations are a different platform and are
  not part of this WFS. No name-marker exclusion is applied; if such a station
  ever appears, it can be filtered by name later.

## Fix design

### 1. New adapter module `backend/src/adapter/driven/leipzig_wfs/`

Follows the Bonn module split. Provider type: **`leipzig_wfs_http_provider`**.

**Config vars** (parsed in `new(&DataSourceConfiguration)`, fail fast on
missing/invalid):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `stations_url` | yes | — | station WFS `GetFeature` URL (`…_standort_statisch`, `outputFormat=application/json`) |
| `hourly_url` | yes | — | hourly time-series WFS URL (`…_anzahl_stunde_zeitreihe`) |
| `daily_url` | yes | — | daily time-series WFS URL (`…_anzahl_tag_zeitreihe`) |
| `max_measurement_batch_size` | no | `500` | page size handed to `get_measurements_source` |
| `cache_duration` | no | `300` | seconds to cache the parsed index |
| `wfs_page_size` | no | `5000` | `count` per WFS page when fetching the time-series layers |

**`fetcher.rs`** — the standard `ResourceFetcher` trait
(`fn fetch(&self, url: &str) -> Result<String, String>`) with a `ureq`-based
`HttpResourceFetcher`, plus a small paging helper `fetch_all_wfs_pages` that
appends `&count=<page>&startIndex=<n>` and follows GeoServer's top-level
`numberMatched` / `numberReturned` (falling back to "keep paging while
`numberReturned == page_size`" when those fields are absent). The **stations**
layer is fetched with a single request (it is small); only the two time-series
layers are paged.

**`parsing.rs`**:
- `utm_zone33n_to_wgs84(easting, northing) -> Option<(f64, f64)>` — inverse
  Transverse Mercator (WGS84/UTM 33N), returning `(latitude, longitude)`. `None`
  for out-of-range values.
- `parse_stations_geojson(&str, messages)` — parses the `FeatureCollection`;
  `stationid` → `external_id`, `stationname` → `name`, UTM point → WGS84
  coordinates (`Point` only, else `None`), timezone `Europe/Berlin`,
  `image_sha256: None`. Features without a resolvable `stationid` are skipped
  (DEBUG).
- `parse_hourly_geojson(&str, messages) -> Vec<HourlyRow>` — parses
  `phenomenontime` (RFC 3339) and `count`; malformed rows skipped (DEBUG).
- `parse_daily_geojson(&str, messages) -> Vec<DailyRow>` — parses the date-only
  `phenomenontime` into a `NaiveDate`; the adapter converts it to UTC via
  `berlin_day_bounds(date) -> (DateTime<Utc>, DateTime<Utc>)` (DST-aware).
- `build_index(stations, hourly_rows, daily_rows, messages) -> LeipzigIndex` —
  one `ChannelRecord` per station (`external_id = counting_station_external_id =
  stationid`), and a `HashMap<String, Vec<MeasurementRecord>>` keyed by
  `stationid`. Hourly rows become `resolution_seconds = 3600, interval_end =
  None`; daily rows become `resolution_seconds = 86400` with the DST-aware
  `interval_end`. Each channel's rows are sorted ascending by timestamp and
  deduplicated on `(timestamp, resolution_seconds)` keep-last (so a daily row and
  the same-instant hourly row are never deduplicated against each other).

**`adapter.rs`** — `LeipzigWfsAdapter` implementing
[`DataProvider`](../backend/src/core/domain/data_source/provider_port.rs:151),
mirroring Bonn:

- In-memory cache `Mutex<Option<CachedData>>` (`fetched_at: Instant` + parsed
  `LeipzigIndex`), refreshed under a `refresh_lock` after `cache_duration`; emits
  an `INFO` lifecycle message on refresh. No persistent state; re-fetching is the
  only refresh strategy.
- `get_all_counting_stations` / `get_all_channels` return the cached records.
- `get_measurements_source(from, max_batch_size)` — reuses
  [`SourceScanner`](../backend/src/adapter/driven/source_merge.rs:48) exactly like
  Bonn: seed the scanner per run, pick the next channel round-robin, serve a
  `ChannelPage` of that channel's rows filtered to `timestamp > from` (exclusive)
  and truncated to `max_batch_size`. Rows are pre-sorted ascending in the index,
  so paging is row-count based only.
- `check_health` — TCP connect to the host/port of `stations_url`.
- `attach_provider_messages` + `emit` — mirrors Bonn.

**Provider messages**:
- `INFO` — one-line lifecycle on refresh (stations/channels/hourly/daily counts).
- `WARNING` — a feature whose `stationid` resolves to no imported station
  (channel/data skipped).
- `DEBUG` — malformed rows / non-point geometry / missing `stationid` (known
  quirks).

### 2. Register the provider

- [`backend/src/adapter/driven/mod.rs`](../backend/src/adapter/driven/mod.rs:1):
  add `pub mod leipzig_wfs;`.
- [`backend/src/adapter/driven/data_provider_factory.rs`](../backend/src/adapter/driven/data_provider_factory.rs:14):
  add a `LeipzigWfsAdapter::provider_type()` match arm + a
  `builds_leipzig_provider_type` test.

### 3. Configuration

- [`config.toml.example`](../config.toml.example:48): add a `[[data_sources]]`
  entry for Leipzig with `stations_url`, `hourly_url`, `daily_url`, and a comment
  documenting the projected-coordinate conversion and the monthly lag.
- The local [`config.toml`](../config.toml:1) (gitignored) gets the same block so
  the running dev stack imports Leipzig.

### 4. Documentation

- [`README.md`](../README.md:1): add Leipzig to the data-source list (provider
  type, vars, the three WFS layers, hourly + daily mixed resolution, UTM→WGS84
  coordinate conversion).
- [`plans/README.md`](../plans/README.md:13): register this plan as the current
  plan.

## File changes

- `backend/src/adapter/driven/leipzig_wfs/mod.rs` (new)
- `backend/src/adapter/driven/leipzig_wfs/adapter.rs` (new)
- `backend/src/adapter/driven/leipzig_wfs/fetcher.rs` (new)
- `backend/src/adapter/driven/leipzig_wfs/parsing.rs` (new)
- `backend/src/adapter/driven/leipzig_wfs/tests.rs` (new)
- `backend/src/adapter/driven/leipzig_wfs/README.md` (new)
- `backend/src/adapter/driven/mod.rs` — module registration
- `backend/src/adapter/driven/data_provider_factory.rs` — factory arm + test
- `config.toml.example` — Leipzig data source (committed template)
- `config.toml` — Leipzig data source (local dev, gitignored)
- `README.md` — docs
- `plans/README.md` — register this plan

No migrations, no core changes, no REST/BFF changes, no frontend changes.

## Testing

Unit tests in `leipzig_wfs/tests.rs` (in-memory mocks + fixture files, per
[`CONTRIBUTING.md`](../CONTRIBUTING.md:157)):

1. Config: missing/invalid `stations_url`/`hourly_url`/`daily_url` → `ConfigError`;
   defaults for `max_measurement_batch_size`, `cache_duration`, `wfs_page_size`.
2. `utm_zone33n_to_wgs84`: assert known Leipzig vectors within a small tolerance —
   `(316626.3846, 5690467.4909)` ≈ `(51.33563, 12.3676)` (Manetstraße),
   `(317893.0312, 5688748.02)` ≈ `(51.32066, 12.38659)` (Semmelweisstraße),
   from the Eco-Visio catalog cross-reference.
3. `parse_stations_geojson`: fixture FeatureCollection → stations with
   `stationid`/`stationname` and converted lon/lat; non-Point geometry → `None`;
   missing `stationid` skipped (DEBUG).
4. `parse_hourly_geojson`: fixture → UTC timestamp (`2026-08-06T00:00:00+02:00` →
   `2026-08-05T22:00:00Z`), `resolution_seconds = 3600`, `interval_end = None`;
   malformed `phenomenontime`/`count` skipped.
5. `parse_daily_geojson` + `berlin_day_bounds`: fixture `2026-03-23` (pre-DST,
   CET) → `timestamp 2026-03-22T23:00:00Z`, `interval_end 2026-03-23T23:00:00Z`;
   a summer date (CEST) shifts both bounds by one hour.
6. `build_index`: one channel per station keyed by `stationid`; hourly and daily
   rows merge into the same channel; a daily row and the same-instant hourly row
   both survive (not deduplicated); duplicate `(timestamp, resolution)` rows
   deduplicate keep-last; rows sorted ascending.
7. `get_measurements_source`: `from` exclusive; mixed-resolution rows served
   ascending; `next_from` watermark behavior identical to Bonn; empty window does
   not advance the cursor.
8. Cache: fresh data reused (fetcher hit once across stations/channels/measurement
   pages); stale refetch (using `cache_duration = "0"`).
9. `check_health`: `Down` for an unreachable host/port.
10. Factory: `builds_leipzig_provider_type`; unknown type still rejected.

Manual validation against the live sources (once, from a shell):

1. `get_all_counting_stations` returns the Leipzig stations with WGS84 coordinates
   (spot-check Manetstraße ≈ `51.3356, 12.3676`).
2. `get_all_channels` returns one channel per station (`external_id = stationid`).
3. Hourly and daily values both import; the first import pages the full history
   without advancing `imported_until` past the latest published sample.
4. A re-run imports nothing new (idempotent upsert on
   `(channel_id, timestamp, resolution_seconds)`).

Gates: `make check`, `make test`, `make test-rest`, `make coverage`.

## Acceptance criteria

- A configured Leipzig data source imports the official stations (WGS84
  coordinates converted from UTM 33N), one channel per station, and both the
  hourly (3600 s) and daily (86400 s, DST-aware `interval_end`) time series.
- Values are never summed or fabricated; mixed resolutions coexist per channel
  without deduping against each other.
- `imported_until` never advances past the latest published sample; an empty
  window leaves it untouched.
- A malformed or unmatched row degrades to a `DEBUG`/`WARNING` message without
  failing the import.
- `make check`, `make test`, `make test-rest`, `make coverage` are green.

## Known trade-offs / follow-ups

- **Re-fetch cost.** The adapter follows Bonn and re-fetches the full layers when
  `cache_duration` elapses (each update run). If the hourly layer proves large,
  add a CQL time filter (`cql_filter=phenomenontime >= '<watermark>'`) or a
  persistent-state cache as a follow-up. This does not block the initial adapter.
- **Monthly publication lag.** `fme_tstamp` shows the layers are published with a
  delay; the `imported_until` watermark absorbs this (most runs import nothing
  new until a fresh batch lands).
