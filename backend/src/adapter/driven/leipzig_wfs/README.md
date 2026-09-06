# Leipzig WFS adapter (`leipzig_wfs`)

Data provider for the **Leipzig** bicycle counters, implemented as a
[`DataProvider`](../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter. It reads the officially published Leipzig WFS layers
(`geodienste.leipzig.de`, GeoServer, `outputFormat=application/json`) over HTTP
and serves counting stations, channels and measurements through the shared
import pipeline. It was added in
[plan 109](../../../../../plans/109_leipzig_wfs_adapter_plan.md:1).

Provider type (the value of `[data_sources.provider].type`):

```
leipzig_wfs_http_provider
```

## Data sources (all official)

The adapter uses the three `radverkehr_dauerzaehlstelle_*` WFS layers:

| Purpose | Layer (`typeName`) | Resolution |
|---|---|---|
| Station locations | `OpenData:radverkehr_dauerzaehlstelle_standort_statisch` | — (one point per station) |
| Hourly counts | `OpenData:radverkehr_dauerzaehlstelle_anzahl_stunde_zeitreihe` | 3600 s |
| Daily counts | `OpenData:radverkehr_dauerzaehlstelle_anzahl_tag_zeitreihe` | 86400 s (calendar-anchored) |

Each layer is a GeoJSON `FeatureCollection`. Measurement features carry
`properties.stationid` (the stable external id / channel key), `stationname`,
`phenomenontime` and `count`; `fme_tstamp` (an FME ingestion stamp) is ignored.

**Data semantics of the source**

- **One channel per station, `external_id = stationid`.** Both the hourly and
  the daily series of a station map to that one channel, at two resolutions that
  coexist in the database (`(channel_id, timestamp, resolution_seconds)` natural
  key).
- **Hourly `phenomenontime`** is RFC 3339 with a numeric UTC offset
  (`+02:00` summer / `+01:00` winter) and is the start of the counted hour.
- **Daily `phenomenontime`** is a date only — a calendar day in Europe/Berlin —
  so its timestamp/`interval_end` are derived DST-aware (a 23 h spring-forward
  day and a 25 h fall-back day are handled correctly).
- **No direction split, no summing** — `count` maps 1:1.
- **Missing data = absent features**, never an explicit zero; genuine `0` values
  are imported.
- **Station coordinates are ETRS89 / UTM zone 33N** (`[easting, northing]`), not
  WGS84 lon/lat; the adapter converts them with a dependency-free inverse
  Transverse Mercator ([`utm_zone33n_to_wgs84`](parsing.rs:199)).

## Configuration

Read from the data source's provider vars in
[`adapter.rs`](adapter.rs:99) (missing/invalid required vars are startup errors):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `stations_url` | yes | — | station WFS `GetFeature` URL |
| `hourly_url` | yes | — | hourly time-series WFS `GetFeature` URL |
| `daily_url` | yes | — | daily time-series WFS `GetFeature` URL |
| `max_measurement_batch_size` | no | `500` | page size |
| `cache_duration` | no | `300` | seconds to cache the parsed index |
| `wfs_page_size` | no | `5000` | `count` per WFS page for the time-series layers |
| `request_timeout_seconds` | no | `30` | end-to-end HTTP request timeout (seconds) |

Example block in [`config.toml.example`](../../../../../config.toml.example).

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`fetcher.rs`](fetcher.rs:1) — `ResourceFetcher` trait (`fetch(&str) -> Result<String, String>`)
  with a `ureq`-based `HttpResourceFetcher`; tests inject a fake.
- [`parsing.rs`](parsing.rs:1) — the UTM 33N→WGS84 conversion, the stations /
  hourly / daily parsers, the WFS paging info, DST-aware `berlin_day_bounds`,
  `paged_url` and `build_index` (mixed-resolution merge + dedup).
- [`adapter.rs`](adapter.rs:1) — `LeipzigWfsAdapter` + the `DataProvider` impl,
  config parsing, WFS paging over the time-series layers, in-memory cache.
- [`tests.rs`](tests.rs:1) — unit tests: config, UTM conversion, parsers,
  paging, merge, serving, cache, health, messages.

## Design decisions

1. **`stationid` is the channel key** for both layers, so hourly and daily rows
   merge into one channel per station. Features without a `stationid` are
   skipped (they cannot be joined to a station).
2. **Mixed resolutions coexist per channel.** Daily rows are calendar-anchored
   (`interval_end` DST-aware); the DB keeps `(channel, timestamp, resolution)`
   distinct and the overlap guard is per-resolution, so a daily row and the
   hourly row at the same local midnight are never lost or overwritten.
3. **Paging truncates at timestamp boundaries.** A channel can hold two rows at
   the same timestamp (different resolutions); a single shared `imported_until`
   watermark can only advance past a timestamp once every row at it is emitted,
   so a page never splits a same-timestamp group (see
   [`page_channel`](adapter.rs:250)).
4. **WFS 2.0 paging** (`count` + `startIndex`) follows the GeoServer
   `numberMatched` / `numberReturned` fields (falling back to "full page ⇒ keep
   paging" when absent), so large time-series layers are fetched in bounded
   requests.
5. **Never sum, never fabricate.** No direction columns exist to sum; missing
   values stay missing.
6. **In-memory cache, no persistent state.** All resources are re-fetched and
   re-parsed when the `cache_duration` elapses (guarded by a refresh lock). The
   `imported_until` watermark keeps incremental runs cheap; because the source
   publishes with a ~1-month lag, most runs find nothing new.
7. **Cursor safety.** `page_channel` filters `timestamp > from` (exclusive) and
   never synthesises a cursor on an empty window.
8. **Health check** is a TCP connect to the host/port of `stations_url`.

## Provider messages

- `INFO` — one-line lifecycle on cache refresh (station/channel/row counts).
- `WARNING` — a measurement whose `stationid` resolves to no imported station
  (channel/data skipped; should not happen with healthy source data).
- `DEBUG` — malformed / non-point-geometry features (known quirks).

## Limitations

- **Full-layer re-fetch per refresh.** Like Bonn, each stale refresh re-downloads
  the whole time-series layers. If the hourly history grows large, add a CQL
  `phenomenontime >= <watermark>` filter or a persistent-state cache (documented
  in plan 109).
- **~1-month publication lag.** The `fme_tstamp` shows the layers are published
  with a delay; `imported_until` absorbs it.
