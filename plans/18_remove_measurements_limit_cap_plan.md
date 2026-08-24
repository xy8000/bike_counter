# 18 - Raise the measurements `limit` default to 5000 and drop the upper cap plan

Status: proposed

## Problem

The measurements read endpoint applies a hard page cap:

```rust
// src/adapter/driving/rest/handlers/mod.rs
const DEFAULT_PAGE_LIMIT: usize = 100;
const MAX_PAGE_LIMIT: usize = 1000;
```

and clamps the request in both [`list_measurements`](../src/adapter/driving/rest/handlers/measurements.rs:26)
and [`list_measurements_raw`](../src/adapter/driving/rest/handlers/measurements.rs:64):

```rust
let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT).min(MAX_PAGE_LIMIT);
```

A single GET therefore returns at most 1000 rows (100 by default), even though
the `measurements` table may hold millions of rows. This cap is unrelated to the
import batching from plan 10 (which bounds provider reads, not DB reads).

## Goal

- Default `limit` to **5000** when the query parameter is not supplied.
- Remove the **upper clamp**: when `limit` is supplied, use it verbatim — a value
  above 5000 is honored.
- Document the 5000 default in the OpenAPI/Swagger description.

## Design

The change stays in the driving adapter; no domain or repository signature
changes are needed (`limit` remains `usize` throughout):

- `DEFAULT_PAGE_LIMIT` becomes `5000`; `MAX_PAGE_LIMIT` is removed.
- Both handlers resolve `limit` as `params.limit.unwrap_or(DEFAULT_PAGE_LIMIT)`
  with no `.min(...)`.
- `MeasurementQueryParams.limit` gains a doc comment so utoipa's `IntoParams`
  emits the default (5000) and the "no upper bound" note in Swagger.

## File changes

- `src/adapter/driving/rest/handlers/mod.rs` — `DEFAULT_PAGE_LIMIT = 5000`,
  remove `MAX_PAGE_LIMIT`, fix the doc comment.
- `src/adapter/driving/rest/handlers/measurements.rs` — drop the
  `.min(MAX_PAGE_LIMIT)` clamp and the `MAX_PAGE_LIMIT` import in both
  `list_measurements` and `list_measurements_raw`.
- `src/adapter/driving/rest/dto/measurements.rs` — add a doc comment to the
  `limit` field of `MeasurementQueryParams` (default 5000, no upper bound) so it
  shows in Swagger.
- `src/adapter/driving/rest/tests/measurements.rs` — update the default-`limit`
  assertions from 100 to 5000 (envelope value and `self` links), and add a test
  proving a `limit` above 5000 is honored (not clamped).
- `README.md` (line 304) — update "capped at 1000 and defaults to 100" to
  "defaults to 5000 with no upper bound".
- `ToDo.md` — update the pagination bullet (remove "handler clamp ≤ 1000").
- `plans/README.md` — summary row updated.

## Acceptance criteria

- `GET /api/v1/measurements` with no `limit` returns up to 5000 rows and reports
  `limit: 5000` in the envelope.
- `GET /api/v1/measurements?limit=10000000` is honored verbatim (no clamp).
- `GET /api/v1/measurements/raw` follows the same default and unbounded behavior.
- Swagger shows the 5000 default and the no-upper-bound note for `limit`.
- `make check`, `make test`, `make test-rest`, and `make coverage` pass.
