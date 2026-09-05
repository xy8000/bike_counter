# Eco-Counter adapter (`eco_counter`)

Eco-Counter (Eco-Visio) bicycle counters, exposed through **three switchable
modes** under the single provider type `eco_counter_http_provider`. A
`[[data_sources]]` entry runs **one or several modes in parallel** through the
comma-separated **`modes`** provider var (`modes = "api_v1"` for a single mode
is fine; `modes = "api_v1, api_v2, screen_scraping"` merges all three behind
one data source).

| Mode | key in `modes` | Source | Station discovery |
|---|---|---|---|
| [`v1`](v1) — legacy publicwebpage API | `api_v1` (default) | `www.eco-visio.net/api/aladdin/1.0.0` | bundled YAML catalog [`v1/stations.yml`](v1/stations.yml) |
| [`v2`](v2) — official API | `api_v2` | `apieco.eco-counter-tools.com/api/1.0` (Bearer access token) | runtime `GET /site` |
| [`scraping`](scraping) — public web view | `screen_scraping` | accessible dashboard page | scaffold (parser not implemented yet) |

## Why three modes

Eco-Counter is migrating tenants off its legacy public API to an API-key-gated
platform, so a single access path no longer covers all counters:

- The **legacy `publicwebpage` API** (V1) still serves the few counters that have
  not migrated, but it no longer auto-discovers German tenants (all German
  cities moved to `*.eco-counter.com` + `api.eco-counter.com/api/v2`), so V1
  uses an explicit per-mode station catalog.
- The **official API** (V2) discovers a whole organisation at runtime and needs
  an organisation-scoped OAuth **access token** (`Authorization: Bearer`).
- The **screen-scraping** mode is where a browser-accessible public view (that
  has no usable API) would be parsed — currently a scaffold.

The [`EcoCounterAdapter`](adapter.rs:1) reads the **`modes`** list only (default
`api_v1` when absent; the older single `mode` var was removed). A single
configured mode is delegated to directly (external ids stay unprefixed). When
**several modes** are listed, the dispatcher wraps them in a
[`Composite`](adapter.rs:1) `DataProvider` that unions their stations/channels
and round-robins their measurement paging in one import run. The modes can
overlap in station ids, so each composite member gets an external-id **prefix**
derived from its mode key (`v1/`, `v2/`, `web/`) that keeps the stations,
channels and measurements of the different access paths apart.

### Mode-scoped vars

The provider vars are a flat string map, so every mode reads its own values
with a mode **var prefix** (`v1_…`, `v2_…`, `web_…`): e.g. the V1 `base_url`
lives in `v1_base_url`, the V2 access token in `v2_access_token`, the scraper
target in `web_scrape_url`. Unset vars fall back to per-mode defaults. Full key
lists are in each mode's `README.md`.

## Config example (all three modes in one data source)

```toml
[[data_sources]]
name = "Eco-Counter"
[data_sources.provider]
type = "eco_counter_http_provider"
[data_sources.provider.vars]
# Run all three modes in parallel behind this one data source. Stations get
# prefixed external ids (v1/100063085, v2/…, web/…). Each mode reads its own
# v1_/v2_/web_ vars from this flat map.
modes = "api_v1, api_v2, screen_scraping"
v2_access_token = "<organisation access token>"   # required by api_v2
# v2_domain_id = "…"          # optional: restrict V2 discovery to one domain
# web_scrape_url = "…"        # optional: page to scrape (default placeholder)
```

A single-mode source is the same with one key:
`modes = "api_v1"` (default; stations from `v1/stations.yml`).

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export of the dispatcher.
- [`adapter.rs`](adapter.rs:1) — `EcoCounterAdapter` dispatcher: parses the
  `modes` list and builds either the single mode provider or the
  [`Composite`](adapter.rs:1) that merges several modes in parallel.
- [`fetcher.rs`](fetcher.rs:1) — shared HTTP abstraction (optional Bearer token).
- [`common.rs`](common.rs:1) — shared UTC/URL helpers.
- [`v1/`](v1) — API_V1 mode (legacy `publicwebpage` API + bundled catalog).
- [`v2/`](v2) — API_V2 mode (official API, Bearer access token).
- [`scraping/`](scraping) — ScreenScraping mode (scaffold).

Each mode has its own `README.md` with the verified API details and config.

## Notes

- Stations are **never** put into the runtime TOML: V1 lists them in
  [`v1/stations.yml`](v1/stations.yml), V2 discovers them at runtime, scraping
  would parse them from the page.
- Verification of the live sources (2026-09-05) is recorded in each mode's
  `README.md`.
