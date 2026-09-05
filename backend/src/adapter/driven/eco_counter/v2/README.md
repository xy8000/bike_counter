# Eco-Counter V2 adapter (`eco_counter` → `v2`)

Data provider for the **official Eco-Counter API**
(`https://apieco.eco-counter-tools.com/api/1.0`), implemented as a
[`DataProvider`](../../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter registered under the provider type **`eco_counter_v2_http_provider`**
(see the parent [`README.md`](../README.md)).

## Source (verified against the live API, 2026-09-05)

- `GET /site` — discovery; returns every site the access token may read, with
  `id`, `name`, `domain`/`domainId`, `latitude`/`longitude`, `timezone`
  (e.g. `(UTC+01:00) Europe/Paris;DST`) and `interval`.
- `GET /site?domain_id=<id>` — discovery restricted to one organisation domain.
- `GET /data/site/{id}?begin=…&end=…&step=hour` — time series as
  `{"date":"2026-08-01T00:00:00+0000","counts":…}` (`date` is the UTC bucket
  start).
- Auth: `Authorization: Bearer <access token>`. Access tokens are **scoped per
  organisation** — to import a city's counters you need a token issued for that
  organisation (obtained from Eco-Counter, https://developers.eco-counter.com/).

## Mapping

- **Station = a site** (`id`), discovered at runtime from `/site` — **no YAML
  catalog**.
- **Channel = one per site** (the site's aggregated series), matching the data
  endpoint's per-site rows. If a site's `counts` arrive as a per-channel
  structure rather than a plain number, those rows are skipped (see
  Limitations).
- Resolution fixed by `step` (`2` = 15 min, `3` = hourly, `4` = daily); bucket
  starts parsed from the UTC `date` field.

## Configuration

Plain (unprefixed) vars from the data source's provider vars:

| Var | Required | Default | Meaning |
|---|---|---|---|
| `access_token` | **yes** | — | the organisation's OAuth access token (`Authorization: Bearer`) |
| `domain_id` | no | — | restrict discovery to one domain (`/site?domain_id=`) |
| `step` | no | `3` | data resolution (`2` = 15 min, `3` = hourly, `4` = daily) |
| `base_url` | no | `https://apieco.eco-counter-tools.com/api/1.0` | official API root |
| `max_measurement_batch_size` | no | `500` | rows kept per source-level batch |
| `cache_duration` | no | `300` | seconds to cache the `/site` discovery |
| `page_days` | no | `7` | day window requested per HTTP call |
| `import_days_back` | no | `365` | initial lookback when no watermark exists |

Example:

```toml
[[data_sources]]
name = "Eco-Counter V2"

[data_sources.provider]
type = "eco_counter_v2_http_provider"

[data_sources.provider.vars]
access_token = "<organisation access token>"
domain_id = "4701"
step = "3"
```

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`client.rs`](client.rs:1) — the **V2 API client** (`OfficialApiClient`): URL
  building + fetching for `/site` and `/data/site/{id}` (the fetcher carries the
  Bearer token).
- [`parsing.rs`](parsing.rs:1) — site/point parsing, timezone extraction, index.
- [`adapter.rs`](adapter.rs:1) — `EcoCounterV2Adapter`: config,
  discovery-cached index, day-window measurement paging over the shared
  [`SourceScanner`](../../../../src/adapter/driven/source_merge.rs:48).
- [`tests.rs`](tests.rs:1) — unit tests (fixtures + fake fetcher).

## Provider messages

- `INFO` — lifecycle on discovery refresh.
- `WARNING` — (reserved for unparseable per-channel `counts`).

## Limitations

- Requires an organisation-scoped access token; a demo/test token only exposes
  its own sites.
- The site series is treated as a single channel; if a site returns per-channel
  `counts`, this adapter does not split them yet.
- Bounded first import (`import_days_back`).

## Testing

Unit tests in [`tests.rs`](tests.rs:1) use fixtures + a fake fetcher. Gates:
`make check`, `make test`, `make test-rest`, `make coverage`.
