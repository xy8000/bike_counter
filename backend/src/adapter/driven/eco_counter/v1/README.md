# Eco-Counter V1 adapter (`eco_counter` → `v1`)

Data provider for **Eco-Counter** bicycle counters that are still served by the
**legacy public Eco-Visio API** (`https://www.eco-visio.net/api/aladdin/1.0.0`),
implemented as a [`DataProvider`](../../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter registered under the provider type **`eco_counter_v1_http_provider`**
(see the parent [`README.md`](../README.md)).

## Source (verified against the live API, 2026-09-05)

The upstream is **in transition**:

- The legacy **tenant discovery** endpoint `publicwebpageplus/{idOrganisme}` is
  **gone for the German tenants** (they migrated to `*.eco-counter.com` +
  `api.eco-counter.com/api/v2`, which requires an API key).
- The per-counter metadata `publicwebpage/{idPdc}` + cumulative data
  `publicwebpage/data/{idPdc}` endpoints **still work without a key** for
  counters that have not migrated. Rows are
  `{"date":"2024-06-01 00:00:00","comptage":481,"timestamp":1717200000000}`
  (epoch-ms `timestamp`, unambiguous; `end` is exclusive).
- German stations can no longer be discovered automatically at runtime, so the
  station list lives in the **bundled YAML catalog**
  [`stations.yml`](stations.yml) (probed on 2026-09-05; five German bicycle
  counters still serve data — see the file's header).

## Mapping

- **Station = a catalog counter (`idPdc`).**
- **Channel = one per station**: the station's **cumulative** series (the site
  total across its directional fields), because the legacy data endpoint returns
  the site total rather than per-direction flows.
- Name/coordinates from the live metadata (`titre`, `latitude`/`longitude`);
  timezone `Europe/Berlin`. Measurement timestamps are the epoch-ms `timestamp`
  (UTC).

## Resolution (smallest first)

The data endpoint serves several resolutions selected by `step` (`2` = 15 min,
`3` = hourly, `4` = daily). The resolution is **not configured**: per channel the
provider probes `[2, 3, 4]` **finest-first** and locks the first step whose day
window returns data. A counter that only serves hourly (or daily) data is
therefore imported at that coarser resolution; every row carries the matching
`resolution_seconds`. The choice is cached per channel and re-probed when the
discovery index is refreshed.

A step is treated as "not available" when its window returns **no rows** or when
the API **rejects it with an HTTP 4xx** — verified live: some counters answer a
finer `step` with `http status: 400` rather than an empty array. Only
transport/server errors are propagated unchanged. If every step is empty at the
served steps the finest step is kept (the counter simply has no data in the
window); a station that rejects **every** step is skipped for the run with a
`WARNING` and never fails the whole import.

## Configuration

Plain (unprefixed) vars from the data source's provider vars. Stations are
**not** in the TOML — they live in [`stations.yml`](stations.yml):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `stations` | no | bundled `stations.yml` | path to a YAML station catalog |
| `base_url` | no | `https://www.eco-visio.net/api/aladdin/1.0.0` | legacy API root |
| `max_measurement_batch_size` | no | `500` | rows kept per source-level batch |
| `cache_duration` | no | `300` | seconds to cache the resolved station index |
| `page_days` | no | `7` | day window requested per HTTP call |
| `import_days_back` | no | `365` | initial lookback when no watermark exists |

Example:

```toml
[[data_sources]]
name = "Eco-Counter"
[data_sources.provider]
type = "eco_counter_v1_http_provider"
[data_sources.provider.vars]
base_url = "https://www.eco-visio.net/api/aladdin/1.0.0"
max_measurement_batch_size = "500"
cache_duration = "300"
page_days = "7"
import_days_back = "365"
```

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`stations.yml`](stations.yml:1) — the station catalog (embedded via `include_str!`).
- [`catalog.rs`](catalog.rs:1) — strict YAML-subset parser for the catalog.
- [`client.rs`](client.rs:1) — the **V1 API client** (`PublicWebpageClient`):
  request building + fetching for `publicwebpage` metadata/data.
- [`parsing.rs`](parsing.rs:1) — response parsing and the station/channel index.
- [`adapter.rs`](adapter.rs:1) — `EcoCounterV1Adapter` (the `DataProvider`
  impl): config, metadata-cached index, resolution probing and day-window
  measurement paging over the shared
  [`SourceScanner`](../../../../src/adapter/driven/source_merge.rs:48).
- [`tests.rs`](tests.rs:1) — unit tests (fixtures + fake fetcher).

## Design decisions

1. **Explicit station catalog, no runtime discovery** (legacy German discovery
   is gone); the operator lists `idPdc` values in
   [`stations.yml`](stations.yml).
2. **Metadata-cached index** — each counter resolved against
   `publicwebpage/{idPdc}` once per `cache_duration`; counters without a token
   are skipped with a `WARNING`; if none resolve the source reports itself
   unreachable.
3. **One channel per station (cumulative)** to avoid double counting.
4. **Finest-first resolution per channel** with coarse-ward fallback (an HTTP
   4xx or an empty window marks a step unavailable).
5. **Day-window paging** — no server-side pagination; each call requests
   `page_days` whole days over the shared `SourceScanner` (safe `imported_until`
   watermark).
6. **Bounded first import** — starts `import_days_back` days ago; raise it to
   backfill further.

## Provider messages

- `INFO` — one-line lifecycle on index refresh; a station imported at a coarser
  resolution than 15 min.
- `WARNING` — a catalog station whose metadata is missing/empty (migrated) or
  that has no `domaine` is skipped.

## Limitations

- **No automatic German tenant discovery** (stations are listed in
  [`stations.yml`](stations.yml)); migrated counters (Bonn, Hessen, …) cannot be
  imported here — use `eco_counter_v2_http_provider` with an Eco-Counter access
  token, or `eco_counter_web_http_provider` to scrape a dashboard.
- **Cumulative series only** (no per-direction channels).
- **Bounded backfill** (`import_days_back`).
- **German timezone assumed** (`Europe/Berlin`).
- Resolution is chosen from the **first usable window** per cache window, so a
  counter whose available resolution changes over time (finer data only
  recently) is read at the coarser resolution that the probe window saw.
- A station that rejects every supported `step` with an HTTP 4xx is skipped with
  a `WARNING` for that run (verified live on the data endpoint) and retried on
  later runs.

## Testing

Unit tests in [`tests.rs`](tests.rs:1) use fixtures + a fake fetcher. Gates:
`make check`, `make test`, `make test-rest`, `make coverage`.
