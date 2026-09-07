# 101 - Split Eco-Counter into three separate adapters (+ V1 multi-resolution)

Status: implemented

## Problem

The Eco-Counter provider currently ships as **one** provider type
(`eco_counter_http_provider`) with **three switchable modes** selected by a
comma-separated `modes` var (`api_v1`, `api_v2`, `screen_scraping`). A single
[`EcoCounterAdapter`](../backend/src/adapter/driven/eco_counter/adapter.rs:283)
dispatcher builds one or several mode providers and — when several are enabled —
wraps them in a [`Composite`](../backend/src/adapter/driven/eco_counter/adapter.rs:58)
that prefixes external ids and round-robins their paging.

That indirection is no longer wanted:

1. Each of the three Eco-Counter access paths should be its **own adapter** with
   its **own provider type** (the provider name must include the version).
2. The `modes` var and the dispatcher/Composite should be removed.
3. The V1 adapter should additionally support **multiple resolutions**, preferring
   the smallest available resolution per counter and falling back to a coarser one
   when the finer resolution has no data.

The running configuration uses exactly the V1 adapter and the web (screen
scraping) adapter; V2 must remain available as a documented, separately
configurable provider type.

## Confirmed provider type names (operator)

- V1: `eco_counter_v1_http_provider`
- V2: `eco_counter_v2_http_provider`
- Web (screen scraping): `eco_counter_web_http_provider`

## Scope

- **In scope:** split the three sub-adapters, remove the `modes` dispatcher and
  its `Composite`, register three provider types, rename config vars to plain
  (unprefixed) names, and add V1 smallest-first multi-resolution selection.
- **Out of scope:** any change to the V2 resolution behaviour (V2 keeps its single
  `step` var), the V1 station catalog contents, the scraping page parser, the
  persistence schema, and the frontend.

## Design decisions

1. **Keep the physical module layout** (`eco_counter/v1`, `eco_counter/v2`,
   `eco_counter/scraping`) but delete the parent dispatcher
   [`eco_counter/adapter.rs`](../backend/src/adapter/driven/eco_counter/adapter.rs:1).
   The shared [`common.rs`](../backend/src/adapter/driven/eco_counter/common.rs:1)
   and [`fetcher.rs`](../backend/src/adapter/driven/eco_counter/fetcher.rs:1)
   stay; each sub-module now owns its adapter identity.
2. **Adapter structs** — rename each provider struct and add `provider_type()`:
   - `EcoCounterV1Provider` → `EcoCounterV1Adapter` (`eco_counter_v1_http_provider`)
   - `EcoCounterV2Provider` → `EcoCounterV2Adapter` (`eco_counter_v2_http_provider`)
   - `EcoCounterScreenScrapingProvider` → `EcoCounterWebAdapter`
     (`eco_counter_web_http_provider`)
   Rename each `provider.rs` → `adapter.rs` to match the Bonn/Hamburg/Münster
   convention.
3. **Drop the mode var prefixes.** Each adapter reads plain vars from the data
   source's provider-vars map; no `modes` var is read anywhere:
   - V1: `base_url`, `stations`, `max_measurement_batch_size`, `cache_duration`,
     `page_days`, `import_days_back` (`step` is removed — see decision 5).
   - V2: `access_token`, `base_url`, `domain_id`, `step`,
     `max_measurement_batch_size`, `cache_duration`, `page_days`,
     `import_days_back`.
   - Web: `scrape_url`, `timezone`, `cache_duration`, `import_days_back`,
     `rate_limit_requests_per_second`, `max_measurement_batch_size`.
4. **Persistent-state keys (web only)** — rename `web_index` / `web_index_at` to
   `index` / `index_at`. This is safe: changing the data source's `provider_type`
   fires the existing `AFTER UPDATE OF provider_type` trigger, which clears the
   source's persistent state, so the old keys are dropped on the next startup and
   the station list is re-scraped once.
5. **V1 multi-resolution** — remove the single `step` var and the
   `resolution_seconds` field. Hardcode the preference order
   **`PREFERRED_STEPS = [2, 3, 4]`** (2 = 15 min → 3 = hourly → 4 = daily). For
   each channel, on its first page, probe the candidate steps in ascending order
   and lock the **first step that returns any usable rows**; if none returns rows,
   keep the finest step (`2`) so an inactive station still pages at the preferred
   resolution. The chosen step is cached per channel (a `Mutex<HashMap<String,
   i64>>`) and cleared whenever the discovery index is refreshed, so resolution is
   re-probed at most once per `cache_duration`. Every row of a page is tagged with
   `resolution_for_step(step)`. A fetch **error** during probing is propagated
   (never treated as "no data"), so network failures are not silently downgraded.
   Emit an `INFO` provider message when a coarser step is selected for a station.
6. **V2 keeps its single `step`** var (default `3` = hourly). The multi-resolution
   requirement is V1-specific.

## Fix design

### 1. V1 adapter

In [`eco_counter/v1`](../backend/src/adapter/driven/eco_counter/v1/mod.rs:1):

- Rename [`provider.rs`](../backend/src/adapter/driven/eco_counter/v1/provider.rs:1)
  → `adapter.rs`; rename `EcoCounterV1Provider` → `EcoCounterV1Adapter`.
- Add `pub fn provider_type() -> &'static str { "eco_counter_v1_http_provider" }`.
- Replace `mode_var` (the `v1_` prefix) with direct
  [`DataSourceConfiguration::provider().var(..)`](../backend/src/core/domain/configuration/configuration.rs:319)
  reads for `base_url`, `stations`, `max_measurement_batch_size`,
  `cache_duration`, `page_days`, `import_days_back`.
- Delete `MODE`, `VAR_PREFIX`, `DEFAULT_STEP`, the `step`/`resolution_seconds`
  fields and the `step()` accessor.
- Add `PREFERRED_STEPS: [i64; 3] = [2, 3, 4]` and a
  `resolutions: Mutex<HashMap<String, i64>>` (channel external id → step).
- Rework `fill_channel_page` to:
  1. look up the locked step for `external_id` (probe if absent);
  2. probe `PREFERRED_STEPS` ascending by calling
     [`client.data`](../backend/src/adapter/driven/eco_counter/v1/client.rs:77)
     with each candidate step and reusing the first non-empty `Vec<RawDataRow>`;
  3. if all candidates are empty, lock `2` and use an empty page;
  4. parse the rows with
     [`resolution_for_step(step)`](../backend/src/adapter/driven/eco_counter/v1/parsing.rs:181)
     and build the [`ChannelPage`](../backend/src/adapter/driven/source_merge.rs:25)
     exactly as today.
- Clear `resolutions` inside `ensure_index` when the index is refreshed.
- Update [`v1/mod.rs`](../backend/src/adapter/driven/eco_counter/v1/mod.rs:9)
  to re-export `EcoCounterV1Adapter` from `adapter` (instead of
  `EcoCounterV1Provider` from `provider`).
- Update [`v1/tests.rs`](../backend/src/adapter/driven/eco_counter/v1/tests.rs:1):
  new struct name, unprefixed vars, provider type string
  `eco_counter_v1_http_provider`, remove `v1_step` assertions, and add resolution
  tests (see Testing).

### 2. V2 adapter

In [`eco_counter/v2`](../backend/src/adapter/driven/eco_counter/v2/mod.rs:1):

- Rename `provider.rs` → `adapter.rs`; rename `EcoCounterV2Provider` →
  `EcoCounterV2Adapter`; add `provider_type()` = `eco_counter_v2_http_provider`.
- Replace `mode_var` (`v2_` prefix) with plain var reads; drop `MODE`/`VAR_PREFIX`.
- Vars: `access_token` (required), `base_url`, `domain_id`, `step`,
  `max_measurement_batch_size`, `cache_duration`, `page_days`, `import_days_back`.
- Update [`v2/mod.rs`](../backend/src/adapter/driven/eco_counter/v2/mod.rs:11)
  and [`v2/tests.rs`](../backend/src/adapter/driven/eco_counter/v2/tests.rs:1).

### 3. Web adapter

In [`eco_counter/scraping`](../backend/src/adapter/driven/eco_counter/scraping/mod.rs:1):

- Rename `provider.rs` → `adapter.rs`; rename `EcoCounterScreenScrapingProvider`
  → `EcoCounterWebAdapter`; add `provider_type()` =
  `eco_counter_web_http_provider`.
- Replace `mode_var` (`web_` prefix) with plain var reads; drop `MODE`/`VAR_PREFIX`.
- Vars: `scrape_url` (required), `timezone`, `cache_duration`,
  `import_days_back`, `rate_limit_requests_per_second`,
  `max_measurement_batch_size`.
- Rename `KEY_INDEX`/`KEY_INDEX_AT` to `index`/`index_at`.
- Update [`scraping/mod.rs`](../backend/src/adapter/driven/eco_counter/scraping/mod.rs:12)
  and [`scraping/tests.rs`](../backend/src/adapter/driven/eco_counter/scraping/tests.rs:1).

### 4. Remove the dispatcher

- Delete [`eco_counter/adapter.rs`](../backend/src/adapter/driven/eco_counter/adapter.rs:1)
  (the `Composite`, `EcoCounterAdapter`, `parse_modes` and its tests).
- Rewrite [`eco_counter/mod.rs`](../backend/src/adapter/driven/eco_counter/mod.rs:1):
  remove `pub use adapter::EcoCounterAdapter;` and `mod adapter;`, re-export the
  three adapters (`pub use v1::EcoCounterV1Adapter;` etc.), and update the module
  docs to describe three independent provider types (no `modes`).

### 5. Factory

In [`data_provider_factory.rs`](../backend/src/adapter/driven/data_provider_factory.rs:18):

- Replace the single `EcoCounterAdapter` arm with three arms, one per new
  `provider_type()`.
- Update imports and replace `builds_eco_counter_provider_type` with per-adapter
  tests (V1 default config; V2 with `access_token`; web with `scrape_url`).

### 6. Configuration

[`config.toml`](../config.toml:61) and
[`config.toml.example`](../config.toml.example:95):

- Rewrite the `Eco-Counter` source as `type = "eco_counter_v1_http_provider"`
  with unprefixed vars (`base_url`, `max_measurement_batch_size`,
  `cache_duration`, `page_days`, `import_days_back`); remove `modes` and `v1_step`.
- Rewrite the `Hessen Mobil` source as `type = "eco_counter_web_http_provider"`
  with unprefixed vars (`scrape_url`, `rate_limit_requests_per_second`,
  `timezone`, `cache_duration`, `import_days_back`); remove `modes`.
- In `config.toml.example`, document a commented `eco_counter_v2_http_provider`
  example (`access_token`, `base_url`, `domain_id`, `step`,
  `max_measurement_batch_size`, `cache_duration`, `page_days`,
  `import_days_back`) and update all surrounding comments.

### 7. Docs

- Rewrite [`eco_counter/README.md`](../backend/src/adapter/driven/eco_counter/README.md:1)
  to list three separate provider types (no `modes`, no `Composite`, no id
  prefixes).
- Update [`v1/README.md`](../backend/src/adapter/driven/eco_counter/v1/README.md:1)
  (unprefixed vars + smallest-first multi-resolution + verified fallback
  behaviour), [`v2/README.md`](../backend/src/adapter/driven/eco_counter/v2/README.md:1)
  and [`scraping/README.md`](../backend/src/adapter/driven/eco_counter/scraping/README.md:1)
  (unprefixed vars).
- Update the Eco-Counter section of the root [`README.md`](../README.md:224).
- Register this plan in [`plans/README.md`](../plans/README.md:1).

## File changes

- `backend/src/adapter/driven/eco_counter/v1/provider.rs` → `adapter.rs` (rename; V1 adapter + multi-resolution)
- `backend/src/adapter/driven/eco_counter/v1/mod.rs` (re-export)
- `backend/src/adapter/driven/eco_counter/v1/tests.rs` (vars/struct/tests)
- `backend/src/adapter/driven/eco_counter/v2/provider.rs` → `adapter.rs` (rename)
- `backend/src/adapter/driven/eco_counter/v2/mod.rs` (re-export)
- `backend/src/adapter/driven/eco_counter/v2/tests.rs`
- `backend/src/adapter/driven/eco_counter/scraping/provider.rs` → `adapter.rs` (rename)
- `backend/src/adapter/driven/eco_counter/scraping/mod.rs` (re-export)
- `backend/src/adapter/driven/eco_counter/scraping/tests.rs`
- `backend/src/adapter/driven/eco_counter/adapter.rs` (delete)
- `backend/src/adapter/driven/eco_counter/mod.rs` (docs + re-exports)
- `backend/src/adapter/driven/data_provider_factory.rs` (three arms + tests)
- `config.toml`
- `config.toml.example`
- `README.md`
- `backend/src/adapter/driven/eco_counter/README.md`
- `backend/src/adapter/driven/eco_counter/v1/README.md`
- `backend/src/adapter/driven/eco_counter/v2/README.md`
- `backend/src/adapter/driven/eco_counter/scraping/README.md`
- `plans/README.md` (register)

No migrations, no core/BFF/REST changes, no frontend changes.

## Testing

Unit tests (fixtures + fake fetcher, no network):

1. **V1 config** — unprefixed vars parse; missing `stations`/defaults work; the
   removed `v1_step` is ignored (no `step()` accessor); invalid values fail with
   `ConfigError`.
2. **V1 multi-resolution** — a channel with 15-min rows uses step `2`
   (`resolution_seconds = 900`); a channel with only hourly rows falls back to
   step `3`; a channel with only daily rows falls back to step `4`; a channel with
   no rows keeps step `2`; mixed channels each get their own resolution; a fetch
   error is propagated, not downgraded; a fallback emits a provider message.
3. **V2 config/provider** — unprefixed `access_token` required; `step` still
   honoured.
4. **Web config/provider** — unprefixed `scrape_url` required; persistent-state
   keys `index`/`index_at` round-trip.
5. **Factory** — each of the three provider types builds; unknown type still
   rejected.
6. Update any test constructors that used `"eco_counter_http_provider"` to the new
   per-adapter types.

Live verification (record results in the V1 README): confirm how the legacy
`publicwebpage/data/{idPdc}` endpoint reports a counter that has no data at a
finer `step` (empty array vs. error) and adjust the fallback trigger accordingly
if the live behaviour differs from "empty rows".

Gates: `make check`, `make test-rest`, `make test`, `make coverage`.

## Acceptance criteria

- There is no `modes` var and no `eco_counter_http_provider` type left in code or
  config.
- Three independent adapters are registered:
  `eco_counter_v1_http_provider`, `eco_counter_v2_http_provider`,
  `eco_counter_web_http_provider`.
- [`config.toml`](../config.toml:61) runs the V1 source and the Hessen Mobil web
  source with the new provider types and unprefixed vars; `config.toml.example`
  documents the V2 source.
- V1 imports each counter at the smallest available resolution, falling back
  coarse-ward per channel, with correct `resolution_seconds` per row.
- The web source's cached discovery still survives a restart under its new
  persistent-state keys after one re-scrape.
- `make check`, `make test-rest`, `make test`, `make coverage` are green.
