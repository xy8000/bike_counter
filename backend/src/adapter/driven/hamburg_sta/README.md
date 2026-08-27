# Hamburg SensorThings adapter (`hamburg_sta`)

Data provider for the **Hamburg** bicycle counters, implemented as a
[`DataProvider`](../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter. It reads the **official Hamburg SensorThings API**
(`https://iot.hamburg.de/v1.0/`) for the dataset
`HH_STA_Verkehrsdaten_Rad_Infrarotdetektoren` — no Eco-Counter, no scraping, no
undocumented endpoints.

Provider type (the value of `[data_sources.provider].type`):

```
hamburg_sta_http_provider
```

## Source (verified against the live API, 2026-08-27)

The live data is published per **`Zählfeld`** (counting field = one direction per
infrared detector) at a **5-minute** resolution. The station-level series
(`Verkehrszählstelle … (veraltet)`) is deprecated and stale (last observations
2026-03-01); it is **not** imported.

- **Current datastreams**: `Rad-Aufkommen an Verkehrszählfeld <F> im 5-Min-Intervall
  am <MQ>`, selected by `properties/layerName eq 'Anzahl_Fahrraeder_Zaehlfeld_5-Min'`.
- **Legacy datastreams** (merged for history by field id):
  `Fahrradaufkommen an Zählfeld <F> im 5-Min-Intervall (veraltet)`
  (service `HH_STA_HamburgerRadzaehlnetz`), which extends each field's 5-min
  history back to ~2025-08.
- A datastream carries `properties.assetID` (field), `properties.knotenName`
  (MQ), `observedArea` (coordinates) and, expanded, the field `richtung`.
- `phenomenonTime` is an interval `[start/end]` (end = start + duration − 1 s).
  `resolution_seconds = end − start + 1 s` (300 for 5-min), `interval_end` is the
  exclusive end.

## Mapping

- **Station = MQ** (measurement cross-section, `knotenName`), ~175 stations.
- **Channel = `Zählfeld`** (one per direction), named `<F> (<richtung>)`.
- Each channel serves the **merged legacy + current 5-min series** (dedup
  keep-last on `(channel, timestamp, resolution)`, current wins) — exactly like
  the Bonn Vortag+yearly merge.

## Configuration

Read from the data source's provider vars in
[`adapter.rs`](adapter.rs:74) (missing/invalid required vars are startup errors):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `base_url` | no | `https://iot.hamburg.de/v1.0/` | SensorThings root |
| `max_measurement_batch_size` | no | `500` | page size |
| `cache_duration` | no | `300` | seconds to cache the discovery index |
| `include_legacy` | no | `true` | merge the `(veraltet)` field series for history |

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`fetcher.rs`](fetcher.rs:1) — `ResourceFetcher` trait + `ureq`-based
  `HttpResourceFetcher`; tests inject a fake.
- [`parsing.rs`](parsing.rs:1) — SensorThings parsing: the rich datastream
  shape (`assetID`/`knotenName`/`observedArea`/expanded `Thing`), the
  `build_index` that groups fields into MQ stations, observation parsing
  (interval → timestamp/resolution/interval_end, sentinel skip) and `build_index`.
- [`adapter.rs`](adapter.rs:1) — `HamburgStaAdapter` + the `DataProvider` impl,
  config parsing, discovery, observation paging, legacy+current merge, health.
- [`tests.rs`](tests.rs:1) — unit tests (fixtures + fake fetcher).

## Design decisions

1. **Discovery is one paginated query.** `Datastreams?$filter=properties/layerName
   eq 'Anzahl_Fahrraeder_Zaehlfeld_5-Min'&$expand=Thing` returns the current and
   legacy field datastreams with coordinates and direction inline (~2 requests),
   so no hardcoded ids and no per-station fetches.
2. **MQ = station, `Zählfeld` = channel.** The `knotenName` property groups the
   directional fields of one cross-section into a station; a field is never
   treated as a station.
3. **Legacy merges into the same field.** The `(veraltet)` field 5-min series
   shares the field id; `include_legacy` merges it so each channel's history
   extends beyond the ~8-week live retention.
4. **Sentinel rows are skipped.** Single-instant `phenomenonTime` observations
   (e.g. a zero at `1990-06-01`) are not imported.
5. **Cursor safety.** `get_measurements` filters `timestamp > from` (exclusive)
   and never synthesises a cursor on an empty window, so `imported_until` never
   jumps into the future (consistent with plan 47).
6. **In-memory cache, no persistent state.** The discovery index is re-fetched
   when `cache_duration` elapses; observations are fetched live per call.
7. **Health check** is a TCP connect to the host/port of `base_url`.

## Provider messages

- `INFO` — one-line lifecycle on discovery refresh (station/channel/field counts).
- `WARNING` — a current datastream without a `knotenName` (no station) is skipped.
- `DEBUG` — a datastream with no resolvable field id is skipped.

## Limitations

- **Live history is limited.** The current feed retains roughly 8 weeks of
  5-min data; the legacy merge extends it to ~2025-08, not to the start of the
  (deprecated) station series in 2020. Long station-level history (daily/weekly)
  is a separate future concern (the official CSV dataset), not part of this
  adapter.
- **Discovery cost.** Discovery fetches all field datastreams (~600) on each
  cache expiry; raise `cache_duration` to reduce load.
- **No station-level data.** The deprecated `Verkehrszählstelle` series is
  intentionally not imported.

## Testing

Unit tests in [`tests.rs`](tests.rs:1) use fixtures + a fake fetcher (no
network). Gates: `make check`, `make test`, `make test-rest`, `make coverage`.
