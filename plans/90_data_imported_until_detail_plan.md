# 90 — Data imported until on the data-source detail page

Status: implemented

## Goal

Expose the persisted `imported_until` import watermark on the BFF data-source
detail payload and render it as a new **"Data imported until"** stat card on the
detail page. This is the first, minimal indication of how far the incremental
import has progressed — everything on/before that timestamp has been imported.

Semantics to keep in mind:

- `imported_until` — the persisted incremental import **cursor** (the flag this
  plan surfaces). It advances batch-by-batch while an import runs.
- `last_data_at` — the newest measurement **actually stored** (max across
  channels). Already in the BFF payload but intentionally not the "imported
  until" indicator.
- `last_updated_at` — wall-clock time of the last **successful** import.

## Backend

- Extend [`BffDataSourceDetailDto`](../backend/src/adapter/driving/bff/dto.rs:713)
  with `pub imported_until: Option<DateTime<Utc>>` (doc: incremental import
  watermark; `null` means "not yet imported").
- Map it in
  [`get_bff_data_source_detail`](../backend/src/adapter/driving/bff/handlers.rs:1129)
  from `detail.data_source.imported_until`.
- Update the BFF test
  [`data_source_detail_returns_stations_and_badge_flags`](../backend/src/adapter/driving/rest/tests/bff.rs:1216)
  to assert `body["imported_until"]` equals `serde_json::Value::Null` (the
  sample source has no watermark yet), mirroring the existing `last_import`
  assertion.

Note: the REST v1 DTO already exposes `imported_until`
([`DataSourceDto`](../backend/src/adapter/driving/rest/dto/data_sources.rs:17));
only the BFF detail DTO is missing it.

## Frontend

- Add `imported_until: string | null` to the
  [`DataSourceDetail`](../frontend/src/features/dataSources/types.ts:36) type.
- In [`DataSourceDetail.tsx`](../frontend/src/features/dataSources/DataSourceDetail.tsx:204)
  add a `StatCard label="Data imported until"` directly after "Last successful
  import", rendering `formatTimestamp(detail.imported_until)`; bump the loading
  skeleton from 8 to 9 ghost cards.

## Tests & gates

- Playwright e2e: extend
  [`data-sources.spec.ts`](../frontend/e2e/data-sources.spec.ts:25) so the detail
  assertion also checks the "Data imported until" label is visible.
- Gates: `make check`, `make test-rest`, `make coverage`,
  `make test-playwright`.
- Docs: register this plan in [`plans/README.md`](../plans/README.md).
