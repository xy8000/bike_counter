# Eco-Counter adapters (`eco_counter`)

Eco-Counter (Eco-Visio) bicycle counters, imported through **three independent
adapters**, each with its **own provider type** and its own `[[data_sources]]`
entry (there is **no `modes` dispatcher** and no composite source):

| Adapter | Provider type (`type =`) | Source | Station discovery |
|---|---|---|---|
| [`v1`](v1) — legacy publicwebpage API | `eco_counter_v1_http_provider` | `www.eco-visio.net/api/aladdin/1.0.0` | bundled YAML catalog [`v1/stations.yml`](v1/stations.yml) |
| [`v2`](v2) — official API | `eco_counter_v2_http_provider` | `apieco.eco-counter-tools.com/api/1.0` (Bearer access token) | runtime `GET /site` |
| [`scraping`](scraping) — public web view | `eco_counter_web_http_provider` | `*.eco-counter.com` dashboard (one tenant per data source) | parsed from the home page (`sites[]`) |

## Why three providers

Eco-Counter is migrating tenants off its legacy public API to an API-key-gated
platform, so a single access path no longer covers all counters:

- The **legacy `publicwebpage` API** (V1) still serves the few counters that have
  not migrated, but it no longer auto-discovers German tenants (all German
  cities moved to `*.eco-counter.com` + `api.eco-counter.com/api/v2`), so V1
  uses an explicit per-station catalog.
- The **official API** (V2) discovers a whole organisation at runtime and needs
  an organisation-scoped OAuth **access token** (`Authorization: Bearer`).
- The **web adapter** parses a browser-accessible public view (that has no
  usable API): the Next.js RSC payloads of the `*.eco-counter.com` dashboards,
  fetching the station list from the home page and each site's daily series from
  its detail page (see [`scraping/README.md`](scraping/README.md)).

## Configuration

Each adapter is a normal `[[data_sources]]` entry that reads **plain, unprefixed
vars** (per version the mode-prefixed `v1_…`/`v2_…`/`web_…` vars and the `modes`
list were removed):

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

- V1 stations are **never** in the TOML — they live in
  [`v1/stations.yml`](v1/stations.yml). Resolution is **not** configured: each
  counter is imported at the **finest resolution that returns data** (15 min,
  else hourly, else daily).
- V2 needs `access_token`; its optional `domain_id`, `step`, `base_url`, … vars
  are documented in [`v2/README.md`](v2/README.md).
- The web adapter needs `scrape_url`; one tenant per data source (see
  [`scraping/README.md`](scraping/README.md)). Example sources are in
  [`config.toml.example`](../../../config.toml.example).

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-exports of the three adapters.
- [`v1/`](v1) — `EcoCounterV1Adapter` (`eco_counter_v1_http_provider`).
- [`v2/`](v2) — `EcoCounterV2Adapter` (`eco_counter_v2_http_provider`).
- [`scraping/`](scraping) — `EcoCounterWebAdapter` (`eco_counter_web_http_provider`).
- [`fetcher.rs`](fetcher.rs:1) — shared HTTP abstraction (optional Bearer token),
  used by V1/V2.
- [`common.rs`](common.rs:1) — shared UTC/URL helpers.

Each adapter has its own `README.md` with the verified API details and config.
The provider types are registered in
[`data_provider_factory.rs`](../../data_provider_factory.rs:17).

## Notes

- Verification of the live sources (2026-09-05) is recorded in each adapter's
  `README.md`.
