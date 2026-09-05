# Eco-Counter API_V1 mode (`eco_counter` → `v1`)

Data provider for **Eco-Counter** bicycle counters that are still served by the
**legacy public Eco-Visio API** (`https://www.eco-visio.net/api/aladdin/1.0.0`),
implemented as a [`DataProvider`](../../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter. This is one of the three switchable modes of the `eco_counter`
adapter (see the parent [`README.md`](../README.md)); it is the **default** mode
(a data source with `modes = "api_v1"` — or no `modes` var — runs this mode).

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
  (UTC); resolution fixed by `step` (`2` = 15 min, `3` = hourly, `4` = daily).

## Configuration

All vars are read with the `v1_` mode prefix from the data source's provider
vars (see [`provider.rs`](provider.rs:92)). Stations are **not** in the TOML —
they live in [`stations.yml`](stations.yml):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `v1_stations` | no | bundled `stations.yml` | path to a YAML station catalog |
| `v1_step` | no | `3` | data resolution (`2` = 15 min, `3` = hourly, `4` = daily) |
| `v1_base_url` | no | `https://www.eco-visio.net/api/aladdin/1.0.0` | legacy API root |
| `v1_max_measurement_batch_size` | no | `500` | rows kept per source-level batch |
| `v1_cache_duration` | no | `300` | seconds to cache the resolved station index |
| `v1_page_days` | no | `7` | day window requested per HTTP call |
| `v1_import_days_back` | no | `365` | initial lookback when no watermark exists |

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`stations.yml`](stations.yml:1) — the station catalog (embedded via `include_str!`).
- [`catalog.rs`](catalog.rs:1) — strict YAML-subset parser for the catalog.
- [`client.rs`](client.rs:1) — the **V1 API client** (`PublicWebpageClient`):
  request building + fetching for `publicwebpage` metadata/data.
- [`parsing.rs`](parsing.rs:1) — response parsing and the station/channel index.
- [`provider.rs`](provider.rs:1) — `EcoCounterV1Provider` (the `DataProvider`
  impl): config, metadata-cached index, day-window measurement paging over the
  shared [`SourceScanner`](../../../../src/adapter/driven/source_merge.rs:48).
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
4. **Day-window paging** — no server-side pagination; each call requests
   `page_days` whole days over the shared `SourceScanner` (safe `imported_until`
   watermark).
5. **Bounded first import** — starts `import_days_back` days ago; raise it to
   backfill further.

## Provider messages

- `INFO` — one-line lifecycle on index refresh.
- `WARNING` — a catalog station whose metadata is missing/empty (migrated) or
  that has no `domaine` is skipped.

## Limitations

- **No automatic German tenant discovery** (stations are listed in
  [`stations.yml`](stations.yml)); migrated counters (Bonn, Hessen, …) cannot be
  imported here — use the `api_v2` mode with an Eco-Counter access token.
- **Cumulative series only** (no per-direction channels).
- **Bounded backfill** (`import_days_back`).
- **German timezone assumed** (`Europe/Berlin`).

## Testing

Unit tests in [`tests.rs`](tests.rs:1) use fixtures + a fake fetcher. Gates:
`make check`, `make test`, `make test-rest`, `make coverage`.
