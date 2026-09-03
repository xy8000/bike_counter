# 96 - Swagger/OpenAPI documentation update

Status: implemented (all gates green)

## Problem

The interactive Swagger-UI (served from
[`openapi.rs`](../backend/src/adapter/driving/rest/openapi.rs:39)) still presents itself as
`Bike Counter REST API` with the description `RESTful API with HATEOAS links and
flat URL hierarchy for Bike Counter Stations`. The backend now serves **two**
HTTP surfaces — the public REST API under `/api/v1` (used for backend-to-backend
integrations) and a Backend-for-Frontend (BFF) API under `/api/bff` consumed by
the React frontend only. The documentation therefore:

- carries a misleading heading ("REST API"),
- lacks concrete `example` values on the BFF schemas (Swagger-UI renders blank
  example boxes),
- has no documentation of the RFC cache headers (`Cache-Control`, `ETag`,
  `If-None-Match` -> `304`) that the BFF actually returns (see
  [`cache.rs`](../backend/src/adapter/driving/bff/cache.rs:1) and
  [`94_frontend_cache_rfc_headers_plan.md`](94_frontend_cache_rfc_headers_plan.md)).

**Scope decision:** keep **all** endpoints (BFF `/api/bff`, the public REST API
`/api/v1`, and the `/health/*` checks) registered exactly as they are. Only the
documentation heading, descriptions, examples and caching-header annotations
change.

## Goal

Make the Swagger-UI describe what the backend actually is:

1. The `info` heading no longer says "REST API" — it documents both HTTP
   surfaces (the frontend-only BFF API and the public REST API for
   backend-to-backend integrations) plus the health checks.
2. The BFF schemas carry useful descriptions and concrete `example` values.
3. Every BFF endpoint documents its `Cache-Control` / `ETag` response headers,
   the `304 Not Modified` revalidation response and the `If-None-Match` request
   header; the asset endpoint additionally documents `Content-Type`,
   `Content-Length` and its origin-dependent `Cache-Control`.

## Approach

### Task 1 — Rename the heading ([`openapi.rs`](../backend/src/adapter/driving/rest/openapi.rs:155))

Change the `info(...)` block:

- `title = "Bike Counter API"` (drop "REST").
- `description` rewritten to state: frontend-only BFF endpoints under
  `/api/bff`, the public REST API under `/api/v1` for backend-to-backend
  integrations, and operational liveness/readiness checks; cacheable BFF JSON
  responses return `Cache-Control` + a strong `ETag` and honor `If-None-Match`.
- Move the `BFF API` tag to the front of the `tags(...)` list so Swagger-UI
  groups the primary interface first.

### Task 2 — BFF DTO descriptions + examples ([`dto.rs`](../backend/src/adapter/driving/bff/dto.rs:1))

- Fill in any missing field-level `///` docs (most are already present).
- Add `#[schema(example = ...)]` to scalar/`Uuid`/`DateTime<Utc>` fields with
  realistic values (e.g. `2026-09-03T12:00:00Z` for timestamps, a UUID for ids).
  Swagger-UI folds these into the generated example bodies of every endpoint
  (verified by an OpenAPI assertion on `GlobalSummaryDto.station_count`).
- **Note:** struct-level `#[schema(example = json!({ ... }))]` object examples
  were tried first, but utoipa 5.5's `ToSchema` derive leaves that attribute
  inert (it is not emitted into the OpenAPI), so examples are provided at the
  field level instead.

### Task 3 — Document caching headers ([`handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:1))

For each BFF `#[utoipa::path(...)]`:

- Add `If-None-Match` to `params(...)` (a `Header` parameter) on the cacheable
  endpoints.
- Add response `headers(...)` to the `200` describing `Cache-Control` (the exact
  policy string per [`cache.rs`](../backend/src/adapter/driving/bff/cache.rs:31)) and `ETag`
  (strong SHA-256 ETag).
- Add a `304 Not Modified` response carrying the `ETag` header for the
  `Windowed` + `ShortLived` endpoints and the asset endpoint.
- Asset endpoint ([`get_bff_asset_content`](../backend/src/adapter/driving/bff/handlers.rs:970)): also
  document `Content-Type`, `Content-Length`, `ETag` and `Cache-Control`
  (`public, max-age=31536000, immutable` for built-ins / `public, max-age=3600`
  for provider images).

**Implementation note:** the exact utoipa 5.5 syntax for response `headers(...)`
(`("Header-Name" = String, description = "...")`) and request `Header` params
(`("If-None-Match" = String, Header, description = "...")`) must be verified
against the installed utoipa version during implementation (a `cargo check`
immediately surfaces any syntax drift).

### Task 4 — Wording alignment

- [`main.rs`](../backend/src/main.rs:301): change the startup log
  `Starting REST API server on ...` -> `Starting Bike Counter API server on ...`.
- [`README.md`](../README.md:402): label the `/api/v1` row as the public REST
  API for backend-to-backend integrations and the `/api/bff` row as the
  frontend-only BFF API; note that Swagger-UI documents both plus the health
  endpoints.

### Task 5 — Tests

- Extend the OpenAPI assertions in
  [`bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:405) (or
  [`root.rs`](../backend/src/adapter/driving/rest/tests/root.rs:40)):
  - assert `body["info"]["title"] == "Bike Counter API"` (no "REST"),
  - assert the `global-summary` `200` response documents `Cache-Control` and
    `ETag` response headers and a `304` response, proving the utoipa header
    annotations are emitted.

## Files touched

| File | Change |
|---|---|
| [`backend/src/adapter/driving/rest/openapi.rs`](../backend/src/adapter/driving/rest/openapi.rs:1) | `info` heading/description, tag order |
| [`backend/src/adapter/driving/bff/dto.rs`](../backend/src/adapter/driving/bff/dto.rs:1) | field docs + `#[schema(example = ...)]` |
| [`backend/src/adapter/driving/bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:1) | utoipa `headers(...)`, `304`, `If-None-Match` |
| [`backend/src/main.rs`](../backend/src/main.rs:301) | startup log wording |
| [`README.md`](../README.md:402) | API table wording |
| [`backend/src/adapter/driving/rest/tests/bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:405) | OpenAPI title/header assertions |

## Definition of done

- [x] Plan registered in [`plans/README.md`](../plans/README.md:1)
- [x] Swagger-UI heading no longer says "REST API"
- [x] BFF schemas show descriptions + examples in Swagger-UI
- [x] BFF endpoints document `Cache-Control`/`ETag`/`304`/`If-None-Match`
- [x] `make check` green
- [x] `make test-rest` green (113)
- [x] `make test` green (557)
- [x] `make coverage` green (overall 86.68%, core 95.07%)
- [x] README / docs wording updated
