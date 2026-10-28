# 165 - Paginate the Import jobs history

Status: implemented

## Problem

Issue #48 asks for pagination in the **Import jobs** table. That table is the
per-data-source import-run history rendered by
[`ImportHistory`](frontend/src/features/dataSources/ImportHistory.tsx:11) on the
data-source detail page. Its BFF endpoint
[`GET /api/bff/data-sources/{id}/imports`](backend/src/adapter/driving/bff/handlers.rs:1395)
currently returns every run in one response, so the list grows without bound.

The default view must show the newest 50 runs, with Previous/Next navigation.
Playwright must cover it.

## Goal

- `GET /api/bff/data-sources/{id}/imports` accepts `offset`/`limit` and returns a
  newest-first page with `total`, `offset`, `limit` and HATEOAS links.
- `offset` defaults to `0`; `limit` defaults to **50**.
- The average import duration stays computed over **all finished runs**, not only
  the visible page.
- `ImportHistory` renders at most 50 rows and offers Previous/Next navigation.
- Playwright proves the default 50-run page and navigation against the seeded
  stack.

## API contract

```http
GET /api/bff/data-sources/{id}/imports?offset=0&limit=50
```

```json
{
  "items": [],
  "total": 55,
  "offset": 0,
  "limit": 50,
  "average_duration_seconds": 30,
  "_links": {
    "self": { "href": "/api/bff/data-sources/{id}/imports?offset=0&limit=50" },
    "next": { "href": "/api/bff/data-sources/{id}/imports?offset=50&limit=50" },
    "root": { "href": "/api/v1" }
  }
}
```

- Ordering is `started_at DESC, id DESC` for stable pagination.
- `next` appears when `offset + items.length < total`.
- `prev` appears when `offset > 0`.
- The response keeps the existing BFF `NoStore` cache policy and `ETag`.

## Implementation tasks

### Task 1 - Domain and repository page

- Add an `ImportRunPage` read model in
  [`import_run.rs`](backend/src/core/domain/data_source/import_run.rs:67):
  `items`, `total`, `average_duration_seconds`.
- Add `DataImportRunRepository::list_page_by_data_source(id, offset, limit)`
  with a default implementation that slices `list_by_data_source` and computes
  the average from the finished runs. This keeps the in-memory test doubles
  compiling.
- Override it in
  [`PostgresImportRunRepository`](backend/src/adapter/driven/postgres/import_run_repository.rs:50):
  - One query returns `COUNT(*)` and the average duration over finished runs.
  - One page query uses `ORDER BY started_at DESC, id DESC LIMIT/OFFSET`.
  - Update the existing `list_by_data_source` ordering with the `id DESC`
    tiebreaker.
- Extend the Postgres repository tests for page windows, total and average.

### Task 2 - Core service

- Add `DataSourceAnalyticsService::imports_page(id, offset, limit)` in
  [`data_source_analytics_service.rs`](backend/src/core/application/data_source_analytics_service.rs:163).
- Add `imports_page` to
  [`DataSourceAnalyticsServicePort`](backend/src/core/domain/data_source_analytics/mod.rs:69).
- Unit-test page windows, total and average with the in-memory run repository.

### Task 3 - BFF endpoint and DTOs

- Extend
  [`BffDataSourceImportsDto`](backend/src/adapter/driving/bff/dto.rs:844) with
  `total`, `offset`, `limit` and `_links`; add a constructor that builds
  self/next/prev/root links for the requested data-source id.
- Add `BffDataSourceImportsQueryParams` (`offset`, `limit`, `IntoParams`).
- Update
  [`get_bff_data_source_imports`](backend/src/adapter/driving/bff/handlers.rs:1411)
  to apply the 0/50 defaults, call `imports_page`, and return the paginated DTO.
- Register `BffDataSourceImportsQueryParams` in the
  [`openapi.rs`](backend/src/adapter/driving/rest/openapi.rs:1) components; the
  handler's `#[utoipa::path(params(...))]` documents the `offset`/`limit` query
  parameters.
- Update the BFF tests: default 50-item page, `offset=50` page with `prev`,
  independent total, and OpenAPI assertions for the `offset`/`limit` query
  parameters plus the paginated `BffDataSourceImportsDto` fields.

### Task 4 - Frontend ImportHistory pagination

- Extend `DataSourceImports` in
  [`types.ts`](frontend/src/features/dataSources/types.ts:41) with `total`,
  `offset` and `limit`.
- Update
  [`fetchDataSourceImports`](frontend/src/features/dataSources/api.ts:16) to
  accept `(id, offset = 0, limit = 50)` and send the query parameters.
- Add internal `offset` state to
  [`ImportHistory`](frontend/src/features/dataSources/ImportHistory.tsx:11),
  reset it when `dataSourceId` changes, and render a footer with
  `Showing 1-50 of N import jobs`, `Page X of Y` and Previous/Next buttons.
- Keep the average-duration label driven by the page response.
- Add `data-testid="import-run-row"` to each run row for Playwright.
- Update/extend `ImportHistory.test.tsx` and `api.test.ts` for the default
  request, paging, error and empty states.

### Task 5 - Playwright and fixture

- Seed **55 finished `data_source_imports` rows for Münster** in both
  [`scripts/dump-e2e-fixture.sh`](scripts/dump-e2e-fixture.sh:82) and the
  committed [`frontend/e2e/e2e-seed.sql`](frontend/e2e/e2e-seed.sql:2713) using
  `generate_series(1, 55)`, 30-second runs and past timestamps. Insert only the
  base columns that exist in the older fixture schema; later migrations add the
  fact columns with defaults.
- Update the data-source detail Playwright spec:
  - The `Import jobs` section now shows the seeded table instead of the empty
    state.
  - The first page renders 50 rows; Previous is disabled; Next is enabled.
  - Next shows the remaining 5 rows; Previous returns to the first page.
  - The BFF response reports `total: 55`, `limit: 50`, `offset: 0`.

### Task 6 - Docs and gates

- Update [`README.md`](README.md:156) with the paginated import-history endpoint.
- Run `make check`, `make test-rest`, `make test`, `make test-unit`,
  `make coverage` and `make test-playwright`.

## Gate results

- `make check` — passed (`cargo fmt --check`, `clippy -D warnings`, Prettier
  and `cargo audit`; audit only reports the allowed `yoke-derive` yank warning).
- `make test-rest` — 132 passed.
- `make test` — 840 passed (includes the new Postgres pagination test).
- `make test-unit` — 91 files / 579 tests passed.
- `make coverage` — passed the backend and frontend thresholds.
- `make test-playwright` — 76 passed, including the new Import jobs pagination
  spec.

## Out of scope

- Paginating the generic `/api/v1/jobs` or `/api/bff/jobs` endpoints.
- Changing the Import jobs table columns or its average-duration definition.

## Definition of done

- [x] `/api/bff/data-sources/{id}/imports` returns newest-first pages with a
      default limit of 50, `total` and next/prev links
- [x] Pagination runs in SQL with a stable `started_at DESC, id DESC` order
- [x] The average duration is calculated over all finished runs, not just the page
- [x] `ImportHistory` renders 50 rows by default and offers Previous/Next
- [x] Playwright covers the default page and navigation against 55 seeded runs
- [x] README and this plan are updated
- [x] Required tests and coverage gates pass
