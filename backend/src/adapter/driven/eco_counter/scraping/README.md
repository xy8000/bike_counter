# Eco-Counter web adapter (`eco_counter` → `scraping`)

The **Eco-Counter web adapter** (provider type **`eco_counter_web_http_provider`**,
see the parent [`README.md`](../README.md)). It imports the counters that
Eco-Counter exposes **only through a public web view** (`*.eco-counter.com`
Next.js dashboards) that has no usable API. **One tenant = one
`[[data_sources]]` entry**, pointed at the tenant root via `scrape_url`.

## Verified page structure (2026-09-05, `https://hessen-mobil.eco-counter.com`)

The dashboards are Next.js App Router apps. Requesting a page with the `RSC: 1`
header makes the server return the React Server Components **Flight payload**
(`text/x-component`) instead of the HTML shell — no `_rsc` token, no JS parsing
and no headless browser are needed. The payload is a stream of newline-separated
records `<id>:<json>`; the data arrays are fully inlined JSON objects:

- **Station list** — `GET {root}/?granularity=P1D&year={current}` embeds the
  tenant's stations under a `"sites":[...]` array (549 stations for Hessen; a
  `bounds_*` viewport is *not* required). Each entry carries `id` (e.g.
  `300027685`), `name` (short code, e.g. `001`), `latitude`/`longitude` +
  `location`, `attributes` (address street/number/postcode/place) and
  `travelModes`. Only `bike` sites are imported.
- **Daily data** — `GET {root}/site/{id}?granularity=P1D&year={YYYY}` embeds the
  site's **daily** series under a `"chartData":[...]` array: one `travelMode`
  (`bike`) entry whose `data[]` holds one point per calendar day at
  **local midnight** (see the DST-offset note below):
  `{"timestamp":"2025-01-01T00:00:00+01:00","traffic":{"counts":18}}`. A year
  request returns the whole calendar year; the current year runs Jan 1 up to
  **yesterday** (the incomplete current day is never exposed).

Each station maps to **one** cumulative daily channel (the site total — even
directional sites expose a single `bike` series). Paging is **year by year**
over the shared [`SourceScanner`](../../../../../src/adapter/driven/source_merge.rs:48).

## Configuration (plain, unprefixed vars)

| Var | Required | Default | Meaning |
|---|---|---|---|
| `scrape_url` | yes | — | the tenant root, e.g. `https://hessen-mobil.eco-counter.com` |
| `rate_limit_requests_per_second` | no | `1` | request rate to the dashboard (`<= 0` disables throttling) |
| `cache_duration` | no | `300` | discovery cache window (seconds) |
| `import_days_back` | no | `365` | initial lookback without an `imported_until` watermark (days) |
| `timezone` | no | `Europe/Berlin` | IANA timezone of the daily series |
| `max_measurement_batch_size` | no | `500` | declared batch size |
| `request_timeout_seconds` | no | `30` | end-to-end HTTP request timeout (seconds) |

Example:

```toml
[[data_sources]]
name = "Hessen Mobil"
[data_sources.provider]
type = "eco_counter_web_http_provider"
[data_sources.provider.vars]
scrape_url = "https://hessen-mobil.eco-counter.com"
rate_limit_requests_per_second = "1"
timezone = "Europe/Berlin"
cache_duration = "3000"
import_days_back = "365"
```

## State & crash safety

- The **measurement watermark** stays with the core's `imported_until`, so a
  crash mid-import loses nothing.
- The **discovered station/channel index** is cached in memory (TTL
  `cache_duration`) and persisted through the adapter-values
  [`PersistentStateAccess`](../../../../../src/core/domain/data_source/provider_port.rs:207)
  store (`index` / `index_at`), so a restart reuses it instead of re-scraping the
  big station list. Measurement data is **daily only**
  (`resolution_seconds = 86400`) with a DST-aware `interval_end`.

## Module layout

- [`fetcher.rs`](fetcher.rs:1) — real page fetcher sending `RSC: 1`.
- [`rate_limit.rs`](rate_limit.rs:1) — minimum-interval rate limiter.
- [`client.rs`](client.rs:1) — URL building + rate-limited page fetches.
- [`parsing.rs`](parsing.rs:1) — RSC record scan, `sites[]` and `chartData[]` parsers.
- [`adapter.rs`](adapter.rs:1) — `EcoCounterWebAdapter` (the `DataProvider`):
  discovery + year paging.
- [`tests.rs`](tests.rs:1) — unit tests against in-repo RSC fixtures.

## Notes / limitations

- A tenant whose RSC payload deduplicates site entries into `"$"` reference
  strings (instead of inlining every object) would need reference resolution;
  Hessen inlines all 549 sites.
- Station images (the `filer.eco-counter-tools.com` URLs in the payload) are not
  imported; stations use the built-in default icon.
- **DST-offset labels.** The dashboard renders each daily point with the UTC
  offset in effect for the date it shows, so the day the clocks spring forward
  is labelled `…T00:00:00+02:00` although its true midnight is still `+01:00`
  (converting the string straight to UTC would put two consecutive days 23 h
  apart and the DB overlap guard would reject the rows). The parser ignores the
  encoded offset and anchors every point at the **true local midnight** of the
  calendar day it names in `timezone`, keeping daily intervals contiguous.
