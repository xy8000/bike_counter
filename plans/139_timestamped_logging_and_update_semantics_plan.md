# 139 - Timestamped backend logs, isolated source failures, and Münster fetch diagnosis plan

Status: implemented

## Problem

On the main server:

1. **Backend logs carry no timestamps.** Logging is plain
   [`println!`/`eprintln!`](backend/src/main.rs:136) at about 110 call sites across
   [`main.rs`](backend/src/main.rs), the [`core`](backend/src/core/application/data_source_update_service.rs:218)
   services and the adapters. Docker only prefixes `bikecounter_backend |`.
2. **Münster "did not fetch" new data.** The source shows `Updated 25.09.26, 20:00`
   / `Last successful import 25.09.26, 20:00` and `Last import Succeeded` (4s,
   0 warnings/errors), but `Data imported until` is stuck at `17.09.26, 23:45`
   even though the `od-ms/radverkehr-zaehlstellen` repo received new data on
   2026-09-25 (about 15h before the 20:00 run). "Updated" = import-completion
   time is the intended behavior and is **kept as-is**.
3. **One failing source fails the whole update job.** The aggregate *"Data source
   update"* job is marked `FAILED` because the unrelated **Eco-Counter** v1 source
   ([`eco_counter_v1_http_provider`](config.toml:55)) fails every run with
   *"no catalog station resolved to a usable counter (all migrated or not public?)"*
   (all five counters in [`stations.yml`](backend/src/adapter/driven/eco_counter/v1/stations.yml:59)
   have migrated).

## Root-cause analysis

### A. Logging

Plain `println!`/`eprintln!`; no framework, no timestamps, no levels, no
filtering.

### B. Aggregate job coupling

[`DataSourceUpdateService::run_updates`](backend/src/core/application/data_source_update_service.rs:297)
collects every per-source error into one aggregate error, so
[`finalize`](backend/src/core/application/data_source_update_service.rs:239)
fails the whole *"Data source update"* job when **any** source fails — including
the Eco-Counter v1 `ProviderError::Unreachable`.

### C. Münster archive fetch (confirmed by live verification)

The four-tier cache in
[`refresh_archive`](backend/src/adapter/driven/muenster_github/adapter.rs:236):

- Tier 1/2 reuse the cached extract/ZIP while within
  [`cache_duration`](backend/src/adapter/driven/muenster_github/adapter.rs:114)
  (default 300s) — **without checking upstream**.
- Tier 4 reuses the ZIP when the upstream HEAD `ETag`/`Last-Modified` still equal
  the persisted headers (otherwise Tier 3 re-downloads).

**Root cause (verified): the archive content the import read was stale — it did
not contain the 18–24.09 measurements that the upstream archive already had.**
The adapter's reuse path (`archive_extracted_at` advanced hourly while
`archive_downloaded_at` stayed at `04:00:03Z`) kept serving that stale ZIP, and no
re-download occurred even though the repo had moved. GitHub's `codeload` branch
archive for `refs/heads/main` is evidently served from a cache that can lag the
branch, so the persisted `ETag` (`W/"f5a3…"`, weak) no longer matches the current
one (`"067e…"`, strong) and the stale content was never replaced. The parser is
**not** at fault: the current archive has 672 rows after 17.09 23:45 for a single
station and the code pages/imports them correctly given fresh content.

## Verification results (live API, 2026-09-25 18:35–18:40 UTC)

- `GET /api/v1/data-sources` — Münster id `a023b021-9754-56c7-8c4e-9c391069aff5`,
  `imported_until = 2026-09-17T21:45:00Z` (= 17.09 23:45 Berlin).
- `GET /api/v1/data-sources/{id}/persistent_state`:
  - `archive_downloaded_at = 2026-09-25T04:00:03Z`
  - `archive_extracted_at = 2026-09-25T18:00:00Z`
  - `archive_etag = W/"f5a3cc8a…"`
- Upstream `main` HEAD commit `bfde7bf` committed `2026-09-25T03:14:27Z`; the
  downloaded archive's CSVs contain data through **2026-09-24 23:45** (Berlin).
- Current upstream `ETag` via HEAD/GET = `"067eebee…"` (strong), i.e. **different**
  from the persisted weak ETag.
- `GET /api/v1/jobs?job_type=data_source_update` — the job runs **hourly** and
  **every** run for ≥3 days is `FAILED` with the Eco-Counter v1 error; Münster's
  own import still runs (provider messages empty because `log_level=WARNING`
  filters the INFO/DEBUG cache messages).

Conclusion: the fetch used stale archive content (not a parser bug and not "the
repo had no new data"), and the per-hour job failures are the Eco-Counter v1
source. Fixes below target both.

## Fix design

### 1. Introduce `tracing` + `tracing-subscriber`

- Add to [`backend/Cargo.toml`](backend/Cargo.toml):
  - `tracing = "0.1"`
  - `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }`
- Initialize the subscriber **first** in [`main.rs`](backend/src/main.rs) (before
  any config read or output) with an RFC 3339 UTC timer and an `EnvFilter`
  defaulting to `info` (override via `RUST_LOG`):
  ```rust
  tracing_subscriber::fmt()
      .with_env_filter(
          tracing_subscriber::EnvFilter::try_from_default_env()
              .unwrap_or_else(|_| "info".into()),
      )
      .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
      .init();
  ```
- Migrate every `println!`/`eprintln!`:
  - `println!` → `tracing::info!`
  - `eprintln!` → `tracing::error!` (or `tracing::warn!` for non-fatal notices)

### 2. Münster fetch diagnostics (tracing)

Add `tracing` lines to the Münster adapter and the import loop so a single run
shows exactly what happened:

- [`refresh_archive`](backend/src/adapter/driven/muenster_github/adapter.rs:236):
  log which tier was chosen and why; log upstream vs persisted
  `ETag`/`Last-Modified` on the Tier-4 decision.
- [`download`](backend/src/adapter/driven/muenster_github/adapter.rs:301):
  log the URL and the returned headers.
- [`build_index`](backend/src/adapter/driven/muenster_github/adapter.rs:384):
  log station/channel counts and the newest CSV file name in the index.
- [`page_channel`](backend/src/adapter/driven/muenster_github/adapter.rs:504):
  log channel id, window, row count, `data_beyond`, `done` (DEBUG level).
- [`DataImportService::update_data_source_with_progress`](backend/src/core/application/data_import_service.rs:387):
  log per-batch `processed`/`added` and the watermark, and the final
  processed/added totals per source.

### 3. Isolate provider failures from the aggregate job

Rework [`run_updates`](backend/src/core/application/data_source_update_service.rs:297)
to capture each failure with its source id, log it, and only treat
**non-provider** errors as fatal:

```rust
let mut fatal = Vec::new();
for (id, result) in results {
    if let Err(error) = result {
        if matches!(error, DomainError::Provider(_)) {
            tracing::error!("data source {id:?} update failed (provider): {error:?}");
        } else {
            fatal.push(format!("{id:?}: {error:?}"));
        }
    }
}
if fatal.is_empty() { Ok(()) } else { Err(DomainError::Provider(/* aggregate */)) }
```

The per-source import run is already recorded `FAILED` with its message in
[`update_one_source`](backend/src/core/application/data_source_update_service.rs:457)
and its `{id}_status` metadata set to `FAILED`, so the data-source detail page
still shows the failure while the aggregate job finishes.

### 4. Münster fetch fix (defense-in-depth)

- Check the production `config.toml` `cache_duration` for the Münster source; if
  it is large, lower it back toward the 300s default.
- Bound the Tier-4 "reuse stale ZIP on matching headers" path with a maximum
  archive age (e.g. 24h): when the persisted ZIP is older than the bound, skip
  the reuse and re-download. This guarantees the archive can never be arbitrarily
  old regardless of header quirks.
- If live verification shows the parser missed rows (format/column/timestamp
  change), fix [`parse_measurement_csv`](backend/src/adapter/driven/muenster_github/parsing.rs:133)
  accordingly.

### 5. Tests

- [`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs)
  tests:
  - New: a provider error in one source does **not** fail the aggregate job (job
    ends `FINISHED`) while that source's import run is `FAILED`.
  - Keep existing DB-failure tests green (they use repository failures, which
    remain fatal).
- [`muenster_github/tests.rs`](backend/src/adapter/driven/muenster_github/tests.rs):
  new test that an over-age ZIP with matching headers is re-downloaded instead of
  reused.

No migrations, no REST/BFF contract changes, no frontend changes.

## File changes

- [`backend/Cargo.toml`](backend/Cargo.toml) — add `tracing`, `tracing-subscriber`.
- [`backend/src/main.rs`](backend/src/main.rs) — subscriber init + macro migration.
- [`backend/src/adapter/**`](backend/src/adapter/mod.rs) — macro migration + Münster
  diagnostics + cache age bound.
- [`backend/src/core/**`](backend/src/core/mod.rs) — macro migration + import-loop
  diagnostics + `run_updates` isolation.
- [`config.toml.example`](config.toml.example) — document the new archive-age bound.

## Deployment recovery (Münster)

The running server still holds a stale cached archive. After deploying, clear the
source's persistent state so the next run re-downloads a fresh archive, then let
the hourly job import the missing days:

```
DELETE /api/v1/data-sources/a023b021-9754-56c7-8c4e-9c391069aff5/persistent_state
```

(Alternatively restart the container; the `max_archive_age` bound re-downloads
within the hour either way.)

## Definition of done

- [x] Live-API verification run; root cause of the Münster miss confirmed and recorded
- [x] Plan file created/updated in [`plans/`](plans)
- [x] `tracing` + `tracing-subscriber` initialized with RFC 3339 UTC timestamps
- [x] All `println!`/`eprintln!` call sites migrated to `tracing` macros
- [x] Münster cache/import diagnostics added (cache tier, headers, per-batch counts/watermark)
- [x] Provider failures no longer fail the aggregate update job; DB failures still do
- [x] Münster archive reuse bounded by a maximum age; production archive age checked
- [x] New tests for isolation and the cache age bound added; existing tests green
- [x] `make check` green
- [x] `make test` green (771 passed)
- [x] `make test-rest` green (128 passed)
- [x] `make coverage` green
- [x] Docs updated ([`agents.md`](agents.md) logging section, adapter README, [`config.toml.example`](config.toml.example))
