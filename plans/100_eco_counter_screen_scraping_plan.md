# 100 - Eco-Counter ScreenScraping mode implementation

Status: in progress

## Problem

The `eco_counter` adapter already has a [`screen_scraping`](backend/src/adapter/driven/eco_counter/scraping/README.md:1)
mode, but it is only a **scaffold**: it reports a healthy/unreachable status and
serves **no stations** (see
[`provider.rs`](backend/src/adapter/driven/eco_counter/scraping/provider.rs:1)).
Eco-Counter migrated many German tenants to `*.eco-counter.com` dashboards that
have **no usable API**, but their data is reachable through public web pages. The
task is to implement the real scraper:

- The **main page** (`https://hessen-mobil.eco-counter.com/`) lists all stations
  and their ids.
- Each **detail page** (`https://hessen-mobil.eco-counter.com/site/300027685`)
  renders graphs whose values can be extracted. Resolution is **daily only**.

The scraper must support **multiple tenants**, each configured by its own root
URL, **survive restarts without data loss** (via the adapter-values persistent
state store), and **rate-limit** requests (default 1 request/second).

## Step 0 - Architecture decisions (confirmed with the user)

1. **Tenant model** — **one `[[data_sources]]` entry per tenant**. The existing
   single `web_scrape_url` var stays; no list var is added. Each tenant becomes
   its own data source:

   ```toml
   [[data_sources]]
   name = "Hessen Mobil"
   [data_sources.provider]
   type = "eco_counter_http_provider"
   [data_sources.provider.vars]
   modes = "screen_scraping"
   web_scrape_url = "https://hessen-mobil.eco-counter.com"
   ```

   Because `modes` has exactly one entry, the
   [`EcoCounterAdapter`](backend/src/adapter/driven/eco_counter/adapter.rs:269)
   dispatcher delegates to the scraping provider directly — external ids stay
   unprefixed, and each tenant has its own `imported_until` watermark and its own
   persistent-state namespace.

2. **Extraction mechanism** — plain HTTP GET, **no JS parsing / no headless
   browser**. The dashboard is a Next.js App Router app whose server-rendered
   resources are reachable by adding `?_rsc=<token>` (React Server Components
   payload):

   - Detail data:
     `GET {root}/site/{id}?granularity=P1D&year={year}&_rsc={token}` returns
     JSON **embedded in the RSC resource** (daily series; `granularity=P1D`).
   - Station list:
     `GET {root}/?granularity=P1D&year={year}&bounds_sw={lat,lng}&bounds_ne={lat,lng}&_rsc={token}`
     returns a **big, mixed body** (not only JSON) — the exact extraction and the
     `_rsc`/bounds strategy are pinned down in **Step 1**.

3. **Resolution** — daily only. Each measurement carries
   `resolution_seconds = 86400` and a DST-aware `interval_end` computed from the
   station timezone. Timezone is configurable (default `Europe/Berlin`).

4. **State** — use the adapter-values KV store
   ([`PersistentStateAccess`](backend/src/core/domain/data_source/provider_port.rs:207),
   exposed per data source) to **cache the discovered station/channel index + a
   scrape cursor** so a restart does not re-scrape every page. The measurement
   watermark stays with the core's `imported_until`. This requires forwarding
   `attach_persistent_state` through the dispatcher/composite (it is currently
   only forwarded for `attach_provider_messages`).

5. **Rate limiting** — a thread-safe limiter in the scraping client, default
   **1 request/second**, configurable via `web_rate_limit_requests_per_second`.
   `HttpResourceFetcher` (shared with v1/v2) stays unchanged.

## Design

### Module shape (mirror v1/v2)

Extend [`scraping/`](backend/src/adapter/driven/eco_counter/scraping) to the same
split used by [`v1`](backend/src/adapter/driven/eco_counter/v1) /
[`v2`](backend/src/adapter/driven/eco_counter/v2):

- `client.rs` — `PageClient` builds the station-list and detail URLs (including
  `granularity`, `year`, `bounds_*`, `_rsc`) and applies the rate limiter before
  every fetch.
- `parsing.rs` — parse the station-list payload into an index (stations +
  channels) and the detail payload into daily `MeasurementRecord`s.
- `provider.rs` — the `DataProvider` (discovery + measurement paging).
- `tests.rs` — unit tests against committed fixtures (no network).

### Station index (one cumulative channel per station)

Like v1, each station maps to **one** channel carrying the station's daily
cumulative series:

- `external_id` = the numeric site id from the page (e.g. `300027685`).
- `name`/`latitude`/`longitude`/`timezone` come from the station payload.
- Discovery result is cached in-memory (TTL `web_cache_duration`, default 300 s)
  **and** persisted through `PersistentStateAccess` so a restart does not re-fetch
  the (large) station list.

### Measurement paging

Reuse [`SourceScanner`](backend/src/adapter/driven/source_merge.rs:48) +
[`ChannelPage`](backend/src/adapter/driven/source_merge.rs:25), exactly as v1/v2
do: the provider round-robins channels and pages each station's daily series in
**year windows** (`?year=YYYY`), advancing each channel's `next_from` per year and
returning the safe `SourceMeasurementBatch.next_from` watermark. The initial seed
is `imported_until`, else `now - web_import_days_back` (default 365 days).

### Rate limiter

A small `RateLimiter { min_interval: Duration, last: Mutex<Option<Instant>> }`
used by `PageClient` before each fetch. `web_rate_limit_requests_per_second` is a
float; `1` = one request/second. Tests construct it with a tiny/zero interval
(or use tolerant timing assertions).

### Persistent state wiring

- Add `attach_persistent_state` forwarding to
  [`EcoCounterAdapter`](backend/src/adapter/driven/eco_counter/adapter.rs:309) and
  to [`Composite`](backend/src/adapter/driven/eco_counter/adapter.rs:127) (mirror
  the existing `attach_provider_messages` forwarding).
- The scraping provider stores the
  [`PersistentStateAccess`](backend/src/core/domain/data_source/provider_port.rs:207)
  handle like
  [`MuensterGithubAdapter`](backend/src/adapter/driven/muenster_github/adapter.rs:58)
  and persists:
  - `web_index` — the discovered index (stations/channels) serialized as JSON.
  - `web_discovery_cursor` — discovery progress (bounds/pagination offset) so a
    crash mid-enumeration resumes.

## Step-by-step plan

### Step 1 - Analysis: station list (live inspection)

- `curl` the root URL with `granularity=P1D`, `year`, `bounds_sw`/`bounds_ne`,
  `_rsc` and inspect the (large, mixed) body.
- Determine:
  - how to obtain/construct the `_rsc` token (extract from the initial HTML, or
    derive it) and whether it is stable per route;
  - the exact station payload (id, name, coordinates) and whether the full set
    is returned in one response or must be enumerated via bounds/pagination;
  - the channel model (one cumulative series vs directional channels).
- Record findings in `scraping/README.md` and save trimmed response fixtures for
  tests.

### Step 2 - Implement station scraping (discovery)

- Add `scraping/parsing.rs` + station-list URL building in `client.rs`.
- Implement `get_all_counting_stations` / `get_all_channels` from the parsed
  index, with in-memory TTL cache + provider messages (`Info` refresh count,
  `Warning` skipped stations).
- Persist/restore the cached index + discovery cursor via `PersistentStateAccess`.
- Unit tests against the fixtures.

### Step 3 - Analysis: station data (live inspection)

- `curl` the detail URL with `granularity=P1D&year=YYYY&_rsc` and inspect the
  embedded JSON.
- Determine the exact daily-value structure (timestamp + count), the paging model
  (one full year per `year`, or a range), and how many years must be fetched for
  the initial backfill.
- Record findings in `scraping/README.md` and save trimmed fixtures.

### Step 4 - Implement station-data scraping

- Parse the daily series into `MeasurementRecord`s
  (`resolution_seconds = 86400`, DST-aware `interval_end`).
- Implement `get_measurements_source` with `SourceScanner` paging (one detail
  fetch per page/year) and `max_measurement_batch_size`.
- Unit tests against the fixtures (timestamp/value parsing, year paging, cursor
  advancement, DST day boundaries).

### Step 5 - Persistent state + dispatcher wiring

- Forward `attach_persistent_state` through `EcoCounterAdapter` and `Composite`.
- Implement the handle storage + load/store in the scraping provider and persist
  `web_index` / `web_discovery_cursor`.
- Tests (adapter forwarding, provider state round-trip, restart restores index).

### Step 6 - Rate limiting

- Add `RateLimiter`, wire into `PageClient`, parse
  `web_rate_limit_requests_per_second` (default `1`).
- Tests for the config default and the limiter behaviour.

### Step 7 - Docs, config and gates

- Update `scraping/README.md`, `eco_counter/README.md`,
  [`config.toml.example`](config.toml.example:95), `README.md`, and register this
  plan in [`plans/README.md`](plans/README.md).
- Run `make check`, `make test`, `make test-rest`, `make coverage`.

## File changes

New:

- `backend/src/adapter/driven/eco_counter/scraping/fetcher.rs`
- `backend/src/adapter/driven/eco_counter/scraping/parsing.rs`
- `backend/src/adapter/driven/eco_counter/scraping/rate_limit.rs`
- `plans/100_eco_counter_screen_scraping_plan.md`

Modified:

- `backend/src/adapter/driven/eco_counter/scraping/client.rs` — URL building + rate limiter
- `backend/src/adapter/driven/eco_counter/scraping/provider.rs` — real discovery + measurements + state
- `backend/src/adapter/driven/eco_counter/scraping/mod.rs` — export `parsing`
- `backend/src/adapter/driven/eco_counter/scraping/tests.rs` — replace scaffold tests
- `backend/src/adapter/driven/eco_counter/scraping/README.md` — verified findings + config table
- `backend/src/adapter/driven/eco_counter/adapter.rs` — forward `attach_persistent_state`
- `backend/src/adapter/driven/eco_counter/README.md` — remove scaffold wording
- `config.toml.example` — `screen_scraping` example (Hessen)
- `plans/README.md` — register this plan

No DB migration is needed: the persistent-state table already exists
([`V4`](backend/migrations/V4__add_data_source_persistent_state.sql:1)) and daily
(`86400`) is already a supported measurement resolution.

## Testing

- Parsing: station list and daily series fixtures (valid rows, malformed rows
  skipped with `WARNING`, missing value/timestamp dropped).
- Provider: discovery cache TTL + persistent-state round-trip; measurement paging
  advances `next_from` per year and never reports a synthetic cursor as the
  watermark.
- Rate limiter: default 1 req/s; zero/tiny interval for functional tests.
- Dispatcher: `attach_persistent_state` reaches the mode provider through both
  the single-mode and composite paths.
- Gates: `make check`, `make test`, `make test-rest`, `make coverage`.

## Acceptance criteria

- A configured `screen_scraping` tenant imports its stations (id/name/coordinates)
  and one daily channel per station, discovered from the live page.
- Daily values are imported with `resolution_seconds = 86400` and DST-aware
  `interval_end`; the hourly/15-min data is not imported.
- Multiple tenants coexist as separate `[[data_sources]]` entries without id
  collisions.
- A restart resumes from the core `imported_until` and the cached discovery index
  without re-scraping every page.
- Requests are rate-limited to the configured value (default 1/second).
- `make check`, `make test`, `make test-rest`, `make coverage` green.

## Implementation status

Steps 0-6 are implemented and green locally (`cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test --bin bike_counter
eco_counter` — 55 tests). The module scrapes the Next.js RSC payloads:

- [`fetcher.rs`](backend/src/adapter/driven/eco_counter/scraping/fetcher.rs:1) —
  `PageFetcher`/`HttpPageFetcher` (sends `RSC: 1`).
- [`rate_limit.rs`](backend/src/adapter/driven/eco_counter/scraping/rate_limit.rs:1) —
  `RateLimiter` (default 1 req/s).
- [`client.rs`](backend/src/adapter/driven/eco_counter/scraping/client.rs:1) —
  `PageClient` URL builders + rate-limited fetches.
- [`parsing.rs`](backend/src/adapter/driven/eco_counter/scraping/parsing.rs:1) —
  RSC record scan; `sites[]` → `SiteIndex`; `chartData[]` → daily values.
- [`provider.rs`](backend/src/adapter/driven/eco_counter/scraping/provider.rs:1) —
  discovery (+ persistent index cache) and year-by-year paging over
  `SourceScanner`, daily `resolution_seconds = 86400` with DST-aware
  `interval_end`.
- [`adapter.rs`](backend/src/adapter/driven/eco_counter/adapter.rs:1) —
  `attach_persistent_state` is now forwarded through `EcoCounterAdapter`/`Composite`.
- Docs/config updated: `scraping/README.md`, `eco_counter/README.md`,
  `eco_counter/mod.rs`, `config.toml.example`.

Remaining: run the Docker-dependent gates (`make test`, `make coverage`,
optionally `make test-playwright`) and validate one tenant end-to-end against a
live dashboard.

## Addendum: live import — DST overlap guard fix (2026-09-05)

The first live Hessen import aborted with
`ERROR: overlapping measurement interval (…, resolution 86400, timestamp
2026-03-28 22:00:00+00)` from the `measurements_no_overlap_guard()` trigger.

**Root cause.** The dashboard renders each daily point with the UTC offset in
effect for the date it shows, so the 2026-03-29 (spring-forward) point is
labelled `2026-03-29T00:00:00+02:00` although its true midnight is still CET
(`+01:00`). The parser stored that instant (UTC `2026-03-28 22:00`) directly,
making consecutive daily rows 23 h apart — their intervals overlap by one hour
and the (unchanged) DB overlap guard rejected the batch.

**Fix (provider-side only — the DB constraint was not touched).**
[`parse_daily_series`](backend/src/adapter/driven/eco_counter/scraping/parsing.rs:1)
is now timezone-aware and **re-anchors every daily point at the true local
midnight of the calendar day it names** (its `…T00:00:00` wall-clock start
interpreted in `web_timezone`), ignoring the possibly DST-shifted offset label.
Consecutive rows are contiguous again (24 h normal days; the DST day keeps its
23/25 h `interval_end`), so the guard accepts them. Callers pass the provider's
timezone; tests fixtures now emit the live dashboard's offset-labelled
timestamps, and a regression test
(`anchors_spring_forward_days_at_true_local_midnight`) pins the behaviour.
Verified against the real 2026 RSC payload (247 daily values, no gaps < 23 h).

Resuming from the persisted `imported_until` (≈ 2025-12-31, past the only stale
2025 autumn rows that had committed) imports Jan–Sep 2026 cleanly without any DB
cleanup.
