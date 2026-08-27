# Münster Open Data GitHub adapter (`muenster_github`)

Data provider for the **Münster** bicycle counters, implemented as a
[`DataProvider`](../../../../src/core/domain/data_source/provider_port.rs:151)
driven adapter. It downloads a ZIP archive published on GitHub, extracts it into
an obscured temp folder, and serves counting stations, channels and measurements
from the extracted files. This was the first adapter and is the reference
implementation for adding new ones (see
[`CONTRIBUTING.md`](../../../../../CONTRIBUTING.md:30)).

Provider type (the value of `[data_sources.provider].type`):

```
münster_opendata_github_provider
```

## Data source

- **URL:** `https://github.com/od-ms/radverkehr-zaehlstellen/archive/refs/heads/main.zip`
- The archive (published by the city of Münster's open-data effort, `od-ms`)
  contains:
  - `site_min.json` — the station/channel index (see [`archive.rs`](archive.rs:8)
    for the verified `ARCHIVE_ROOT` and `SITE_INDEX_FILE` names).
  - one folder per counting station, each with **monthly CSVs** named `YYYY-MM.csv`
    (e.g. `300038855/2023-01.csv`). Each CSV column header is `<channel-id>
    (<channel-name>)`; the first column is a **local Europe/Berlin** timestamp.
- The ZIP is a snapshot of the `main` branch — a moving target that is
  re-downloaded when the cache expires.

## Configuration

Read from the data source's provider vars in
[`adapter.rs`](adapter.rs:82) (missing/invalid required vars are startup errors):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `url` | yes | — | ZIP archive URL (GitHub `main` branch) |
| `max_measurement_batch_size` | no | `500` | page size |
| `max_measurement_timeframe_hours` | no | `168` (7 days) | import time window per provider call |
| `cache_duration` | no | `300` | seconds an extracted/downloaded archive stays fresh |

Example block in [`config.toml.example`](../../../../../config.toml.example:25).

## Module layout

- [`mod.rs`](mod.rs:1) — module docs + re-export.
- [`adapter.rs`](adapter.rs:1) — `MuensterGithubAdapter` + the `DataProvider` impl,
  config parsing, the four-tier archive cache lifecycle, measurement serving.
- [`fetcher.rs`](fetcher.rs:1) — HTTP abstraction (`ArchiveFetcher` trait,
  `HttpFetcher` via `ureq`): `head` (ETag/Last-Modified) and `get` (download).
- [`archive.rs`](archive.rs:1) — in-memory `ArchiveIndex` and zip-path safety.
- [`parsing.rs`](parsing.rs:1) — `site_min.json` + monthly-CSV parsers,
  timezone/url helpers.
- [`station_metadata.rs`](station_metadata.rs:1) — hardcoded station names +
  GPS coordinates (the archive has none).
- [`tests.rs`](tests.rs:1) — unit tests (fixtures + fake fetcher).

## Design decisions

1. **Four-tier archive cache** (in [`adapter.rs`](adapter.rs:228)),
   serialized under a refresh lock and tracked via the scoped
   [`PersistentStateAccess`](../../../../src/core/domain/data_source/provider_port.rs:191)
   handle (keys `archive_downloaded_at`, `archive_extracted_at`,
   `archive_file`, `archive_extracted_dir`, `archive_etag`,
   `archive_last_modified`):
   - **Tier 1** — extracted folder exists and is fresh → reuse it.
   - **Tier 2** — ZIP fresh but folder missing → re-extract from the ZIP.
   - **Tier 3** — (re-)download + extract.
   - **Tier 4** — best-effort upstream-change detection: if the upstream
     `ETag`/`Last-Modified` are unchanged, reuse the stale ZIP and re-extract.
2. **Zip-slip safety.** Every entry name goes through
   [`sanitize_zip_path`](archive.rs:23), which rejects anything that would escape
   the extraction directory. Extraction goes to a fresh obscured temp dir.
3. **Aggregate entry is skipped.** `parse_site_index` drops the station aggregate
   column (`id == directory`) so the global summary is not double-counted.
4. **Naming invariants enforced defensively.** Channel names are made unique
   within their station and station names unique within the archive by appending
   the external id when the upstream data is not already unique (e.g. `Bohlweg
   Fahrräder Stadteinwärts (353484923)`), so imports never violate the V8
   uniqueness constraints.
5. **Hardcoded station metadata.** The archive's `site_min.json` carries no GPS
   coordinates, so [`station_metadata.rs`](station_metadata.rs:19) hardcodes the
   canonical name + WGS84 coordinates for the known stations (keyed by external
   id). Unlisted stations get "not provided" coordinates, patchable later via the
   counting-stations API.
6. **Timeframe-bounded paging with cursor safety.** `get_measurements` pages
   through monthly files overlapping the current window (`windowed_series`) and
   bounds each provider call to `max_measurement_timeframe_hours`. When the
   window holds no rows and no later data exists, the cursor is **not** advanced —
   otherwise `imported_until` would jump into the future and silently skip data
   that arrives later (plan 47).
7. **Missing channel column is a known quirk, not a failure.** A channel absent
   from a monthly file emits a `DEBUG` message (below the default `WARNING` log
   level) and returns an empty batch so the import continues; genuine IO/parse
   errors still fail the job.
8. **DST-aware timestamps.** Monthly CSV timestamps are local Europe/Berlin and
   converted to UTC with `single()` → `earliest()` on the ambiguous DST hour.
9. **Health check** is a TCP connect to `github.com:443` (the archive host).

## Provider messages

- `INFO` — lifecycle events on download/extract/cache reuse.
- `DEBUG` — cache-freshness decisions and the missing-column quirk (kept below
  the default `WARNING` log level to avoid per-file noise).
- Message recording is best-effort; store failures are swallowed so they never
  break data serving.

## Limitations

- **Depends on GitHub and the `main` branch ZIP** — the source is a moving
  snapshot, so an import needs outbound GitHub access and a stable network. The
  download is re-triggered whenever the cache expires and headers change.
- **No images in the archive** — stations fall back to the built-in default
  image.
- **GPS coordinates are hardcoded** in
  [`station_metadata.rs`](station_metadata.rs:19); new/renamed stations not in
  the table get "not provided" coordinates until patched via the API.
- **Temp-dir extraction** — the extracted archive lives in the system temp
  folder; a large archive consumes disk until the next refresh cleans up
  (obscured per-refresh dirs).
- **Channel absence in a file is tolerated (DEBUG)** — a channel that disappears
  from newer monthly files yields no data for those months without an obvious
  error, by design.
- **License**: see the upstream `od-ms/radverkehr-zaehlstellen` repository; the
  data is published as open data by the city of Münster. The adapter only reads
  the public archive and performs no scraping.

## Testing

Unit tests in [`tests.rs`](tests.rs:1) use fixtures and a fake fetcher (no
network, no GitHub). They cover config parsing, the site index parser, monthly
CSV parsing (DST), archive caching tiers, windowed paging and the cursor-safety
behaviour. Gates: `make check`, `make test`, `make test-rest`, `make coverage`.
